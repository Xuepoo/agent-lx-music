#![allow(clippy::arc_with_non_send_sync, clippy::collapsible_if)]
use anyhow::{Result, anyhow};
use rquickjs::{Context, Function, Runtime, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Wall-clock budget for a single source-script execution. Scripts exceeding
/// it are interrupted via the QuickJS interrupt handler.
pub const SCRIPT_DEADLINE_SECS: u64 = 30;

/// QuickJS heap ceiling per sandbox instance. Runaway allocations inside a
/// source script fail with an out-of-memory error instead of exhausting the
/// host process.
pub const MAX_HEAP_BYTES: usize = 256 * 1024 * 1024;

/// Upper bound on pending microtask jobs drained per execution. Protects
/// against endless promise chains that would otherwise spin the event loop.
const MAX_PENDING_JOBS: u32 = 100_000;

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
pub struct SandboxState {
    pub inited_data: Option<serde_json::Value>,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub homepage: String,
    pub raw_script: String,
}

pub struct JsSandbox {
    pub context: Option<Context>,
    pub runtime: Runtime,
    deadline_secs: u64,
    /// Absolute deadline (unix millis); 0 means disarmed.
    deadline_ms: Arc<AtomicU64>,
}

impl JsSandbox {
    pub fn new() -> Result<Self> {
        Self::build(SCRIPT_DEADLINE_SECS)
    }

    #[cfg(test)]
    pub fn with_deadline(deadline_secs: u64) -> Result<Self> {
        Self::build(deadline_secs)
    }

    fn build(deadline_secs: u64) -> Result<Self> {
        let runtime = Runtime::new()?;
        runtime.set_memory_limit(MAX_HEAP_BYTES);
        let context = Context::full(&runtime)?;
        let deadline_ms = Arc::new(AtomicU64::new(0));
        let deadline = deadline_ms.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let deadline = deadline.load(Ordering::Relaxed);
            deadline != 0 && unix_ms() >= deadline
        })));
        Ok(Self {
            context: Some(context),
            runtime,
            deadline_secs,
            deadline_ms,
        })
    }

    fn arm_deadline(&self) {
        self.deadline_ms.store(
            unix_ms().saturating_add(self.deadline_secs.saturating_mul(1000)),
            Ordering::Relaxed,
        );
    }

    fn disarm_deadline(&self) {
        self.deadline_ms.store(0, Ordering::Relaxed);
    }

    fn deadline_hit(&self) -> bool {
        let deadline = self.deadline_ms.load(Ordering::Relaxed);
        deadline != 0 && unix_ms() >= deadline
    }

    fn timeout_err(&self) -> anyhow::Error {
        anyhow!("script execution timed out after {}s", self.deadline_secs)
    }

    /// Map any error observed while the wall-clock deadline has been exceeded
    /// to a deterministic timeout error (interrupted scripts surface as
    /// generic QuickJS exceptions).
    fn map_deadline_error(&self, res: Result<()>) -> Result<()> {
        match res {
            Err(_) if self.deadline_hit() => Err(self.timeout_err()),
            other => other,
        }
    }

    /// Drive pending microtask jobs bounded by both an iteration cap and the
    /// execution wall-clock deadline.
    fn drive_pending_jobs(&self) -> Result<()> {
        for _ in 0..MAX_PENDING_JOBS {
            match self.runtime.execute_pending_job() {
                Ok(true) => {
                    if self.deadline_hit() {
                        return Err(self.timeout_err());
                    }
                }
                Ok(false) => return Ok(()),
                Err(_) if self.deadline_hit() => return Err(self.timeout_err()),
                Err(e) => {
                    return Err(anyhow!("JS pending-job execution failed: {e}"));
                }
            }
        }
        Err(anyhow!(
            "script exceeded the pending-job budget ({MAX_PENDING_JOBS} jobs)"
        ))
    }

    pub fn execute_resolve(
        &self,
        script: &str,
        platform: &str,
        _song_id: &str,
        quality: &str,
        music_info: serde_json::Value,
    ) -> Result<String> {
        let resolved_data = Arc::new(Mutex::new(None));
        let resolved_err = Arc::new(Mutex::new(None));

        self.arm_deadline();
        let with_res = self.context.as_ref().unwrap().with(|ctx| -> Result<()> {
            let meta = super::loader::parse_metadata(script).ok();
            let mut state_obj = SandboxState::default();
            if let Some(m) = meta {
                state_obj.name = m.name;
                state_obj.description = m.description.unwrap_or_default();
                state_obj.version = m.version.unwrap_or_default();
                state_obj.author = m.author.unwrap_or_default();
                state_obj.homepage = m.homepage.unwrap_or_default();
            }
            state_obj.raw_script = script.to_string();
            let state = Arc::new(Mutex::new(state_obj));

            let res = (|| -> Result<()> {
                super::bridge::inject_lx(&ctx, state.clone())?;

                // Evaluate the source JS script
                ctx.eval::<(), _>(script)
                    .map_err(|e| anyhow!("Failed to evaluate JS script: {}", e))?;

                let handler: Function = ctx
                    .globals()
                    .get("__lx_request_handler")
                    .map_err(|_| anyhow!("JS source did not register a request handler"))?;

                // Construct JS request arguments
                let js_arg = serde_json::json!({
                    "action": "musicUrl",
                    "source": platform,
                    "info": {
                        "type": quality,
                        "musicInfo": music_info
                    }
                });

                // Invoke request handler and get return value
                let js_val_arg = ctx.json_parse(serde_json::to_string(&js_arg)?)?;
                let promise_val: Value = handler.call((js_val_arg,))?;

                // If it returns a Promise, we await/resolve it using a local event loop
                if let Some(obj) = promise_val.as_object() {
                    if obj.contains_key("then")? {
                        let success_cb = Function::new(ctx.clone(), {
                            let r = resolved_data.clone();
                            move |val: Value| {
                                if let Some(text) = val.as_string().and_then(|s| s.to_string().ok())
                                {
                                    let mut guard = r.lock().unwrap();
                                    *guard = Some(text);
                                } else {
                                    // If not a string, try JSON formatting or string fallback
                                    let mut guard = r.lock().unwrap();
                                    if let Some(obj) = val.as_object() {
                                        if let Ok(msg) = obj.get::<_, String>("message") {
                                            *guard = Some(msg);
                                            return;
                                        }
                                    }
                                    *guard = Some(format!("{:?}", val));
                                }
                            }
                        })?;

                        let fail_cb = Function::new(ctx.clone(), {
                            let e = resolved_err.clone();
                            move |val: Value| {
                                if let Some(text) = val.as_string().and_then(|s| s.to_string().ok())
                                {
                                    let mut guard = e.lock().unwrap();
                                    *guard = Some(text);
                                } else {
                                    let mut guard = e.lock().unwrap();
                                    if let Some(obj) = val.as_object() {
                                        if let Ok(msg) = obj.get::<_, String>("message") {
                                            *guard = Some(msg);
                                            return;
                                        }
                                    }
                                    *guard = Some(format!("{:?}", val));
                                }
                            }
                        })?;

                        // Resolve the promise using a JS arrow function to ensure correct 'this' binding
                        let promise_helper: Function = ctx.eval(
                            " (promise, success, fail) => { promise.then(success, fail); } ",
                        )?;
                        let _: Value = promise_helper.call((obj.clone(), success_cb, fail_cb))?;
                    } else {
                        let text = promise_val
                            .as_string()
                            .ok_or_else(|| anyhow!("Handler did not return a string or promise"))?
                            .to_string()?;
                        let mut guard = resolved_data.lock().unwrap();
                        *guard = Some(text);
                    }
                } else if let Some(s) = promise_val.as_string() {
                    let text = s.to_string()?;
                    let mut guard = resolved_data.lock().unwrap();
                    *guard = Some(text);
                } else {
                    return Err(anyhow!("Handler returned unsupported value type"));
                };

                // Clear globals handler reference to ensure no leaks
                let _ = ctx
                    .globals()
                    .set("__lx_request_handler", Value::new_null(ctx.clone()));

                Ok(())
            })();

            if let Err(ref e) = res {
                let catch_val = ctx.catch();
                if !catch_val.is_null() && !catch_val.is_undefined() {
                    if let Some(obj) = catch_val.as_object() {
                        if let Ok(msg) = obj.get::<_, String>("message") {
                            return Err(anyhow!(
                                "QuickJS Exception: {} (underlying error: {})",
                                msg,
                                e
                            ));
                        }
                    }
                    return Err(anyhow!(
                        "QuickJS Exception: {:?} (underlying error: {})",
                        catch_val,
                        e
                    ));
                }
            }
            res
        });
        let with_res = self.map_deadline_error(with_res);
        let drained = self.drive_pending_jobs();
        self.disarm_deadline();
        with_res?;
        drained?;

        if let Some(err) = resolved_err.lock().unwrap().clone() {
            return Err(anyhow!("Promise rejected: {}", err));
        }

        resolved_data
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("Promise did not resolve to a string url"))
    }

    pub fn execute_init(&self, script: &str) -> Result<serde_json::Value> {
        let inited_data = Arc::new(Mutex::new(None));

        self.arm_deadline();
        let with_res = self.context.as_ref().unwrap().with(|ctx| -> Result<()> {
            let meta = super::loader::parse_metadata(script).ok();
            let mut state_obj = SandboxState::default();
            if let Some(m) = meta {
                state_obj.name = m.name;
                state_obj.description = m.description.unwrap_or_default();
                state_obj.version = m.version.unwrap_or_default();
                state_obj.author = m.author.unwrap_or_default();
                state_obj.homepage = m.homepage.unwrap_or_default();
            }
            state_obj.raw_script = script.to_string();
            let state = Arc::new(Mutex::new(state_obj));

            let res = (|| -> Result<()> {
                super::bridge::inject_lx(&ctx, state.clone())?;

                ctx.eval::<(), _>(script)
                    .map_err(|e| anyhow!("Failed to evaluate JS script during init: {}", e))?;

                let mut inited_guard = inited_data.lock().unwrap();
                *inited_guard = state.lock().unwrap().inited_data.clone();
                Ok(())
            })();

            if let Err(ref e) = res {
                let catch_val = ctx.catch();
                if !catch_val.is_null() && !catch_val.is_undefined() {
                    if let Some(obj) = catch_val.as_object() {
                        if let Ok(msg) = obj.get::<_, String>("message") {
                            return Err(anyhow!(
                                "QuickJS Exception: {} (underlying error: {})",
                                msg,
                                e
                            ));
                        }
                    }
                    return Err(anyhow!(
                        "QuickJS Exception: {:?} (underlying error: {})",
                        catch_val,
                        e
                    ));
                }
            }
            res
        });
        let with_res = self.map_deadline_error(with_res);
        let drained = self.drive_pending_jobs();
        self.disarm_deadline();
        with_res?;
        drained?;

        let inited = inited_data.lock().unwrap().clone();
        inited.ok_or_else(|| anyhow!("JS source did not invoke lx.send('inited', ...)"))
    }

    pub fn execute_lyric(
        &self,
        script: &str,
        platform: &str,
        _song_id: &str,
        music_info: serde_json::Value,
    ) -> Result<String> {
        let resolved_data = Arc::new(Mutex::new(None));
        let resolved_err = Arc::new(Mutex::new(None));

        self.arm_deadline();
        let with_res = self.context.as_ref().unwrap().with(|ctx| -> Result<()> {
            let meta = super::loader::parse_metadata(script).ok();
            let mut state_obj = SandboxState::default();
            if let Some(m) = meta {
                state_obj.name = m.name;
                state_obj.description = m.description.unwrap_or_default();
                state_obj.version = m.version.unwrap_or_default();
                state_obj.author = m.author.unwrap_or_default();
                state_obj.homepage = m.homepage.unwrap_or_default();
            }
            state_obj.raw_script = script.to_string();
            let state = Arc::new(Mutex::new(state_obj));

            let res = (|| -> Result<()> {
                super::bridge::inject_lx(&ctx, state.clone())?;

                // Evaluate the source JS script
                ctx.eval::<(), _>(script)
                    .map_err(|e| anyhow!("Failed to evaluate JS script for lyrics: {}", e))?;

                let handler: Function = ctx
                    .globals()
                    .get("__lx_request_handler")
                    .map_err(|_| anyhow!("JS source did not register a request handler"))?;

                // Construct JS lyric request arguments
                let js_arg = serde_json::json!({
                    "action": "lyric",
                    "source": platform,
                    "info": {
                        "musicInfo": music_info
                    }
                });

                // Invoke request handler and get return value
                let js_val_arg = ctx.json_parse(serde_json::to_string(&js_arg)?)?;
                let promise_val: Value = handler.call((js_val_arg,))?;

                // Resolve promise or get string
                if let Some(obj) = promise_val.as_object() {
                    if obj.contains_key("then")? {
                        let success_cb = Function::new(ctx.clone(), {
                            let r = resolved_data.clone();
                            move |val: Value| {
                                if let Some(text) = val.as_string().and_then(|s| s.to_string().ok())
                                {
                                    let mut guard = r.lock().unwrap();
                                    *guard = Some(text);
                                } else {
                                    let mut guard = r.lock().unwrap();
                                    if let Some(obj) = val.as_object() {
                                        if let Ok(msg) = obj.get::<_, String>("message") {
                                            *guard = Some(msg);
                                            return;
                                        }
                                    }
                                    *guard = Some(format!("{:?}", val));
                                }
                            }
                        })?;

                        let fail_cb = Function::new(ctx.clone(), {
                            let e = resolved_err.clone();
                            move |val: Value| {
                                if let Some(text) = val.as_string().and_then(|s| s.to_string().ok())
                                {
                                    let mut guard = e.lock().unwrap();
                                    *guard = Some(text);
                                } else {
                                    let mut guard = e.lock().unwrap();
                                    if let Some(obj) = val.as_object() {
                                        if let Ok(msg) = obj.get::<_, String>("message") {
                                            *guard = Some(msg);
                                            return;
                                        }
                                    }
                                    *guard = Some(format!("{:?}", val));
                                }
                            }
                        })?;

                        let promise_helper: Function = ctx.eval(
                            " (promise, success, fail) => { promise.then(success, fail); } ",
                        )?;
                        let _: Value = promise_helper.call((obj.clone(), success_cb, fail_cb))?;
                    } else {
                        let text = promise_val
                            .as_string()
                            .ok_or_else(|| {
                                anyhow!("Handler did not return a string or promise for lyrics")
                            })?
                            .to_string()?;
                        let mut guard = resolved_data.lock().unwrap();
                        *guard = Some(text);
                    }
                } else if let Some(s) = promise_val.as_string() {
                    let text = s.to_string()?;
                    let mut guard = resolved_data.lock().unwrap();
                    *guard = Some(text);
                } else {
                    let helper_fn: Function = ctx.eval(" (val) => JSON.stringify(val) ")?;
                    let stringified: String = helper_fn.call((promise_val,))?;
                    let mut guard = resolved_data.lock().unwrap();
                    *guard = Some(stringified);
                };

                // Clear globals handler reference to ensure no leaks
                let _ = ctx
                    .globals()
                    .set("__lx_request_handler", Value::new_null(ctx.clone()));

                Ok(())
            })();

            if let Err(ref e) = res {
                let catch_val = ctx.catch();
                if !catch_val.is_null() && !catch_val.is_undefined() {
                    if let Some(obj) = catch_val.as_object() {
                        if let Ok(msg) = obj.get::<_, String>("message") {
                            return Err(anyhow!(
                                "QuickJS Exception: {} (underlying error: {})",
                                msg,
                                e
                            ));
                        }
                    }
                    return Err(anyhow!(
                        "QuickJS Exception: {:?} (underlying error: {})",
                        catch_val,
                        e
                    ));
                }
            }
            res
        });
        let with_res = self.map_deadline_error(with_res);
        let drained = self.drive_pending_jobs();
        self.disarm_deadline();
        with_res?;
        drained?;

        if let Some(err) = resolved_err.lock().unwrap().clone() {
            return Err(anyhow!("Promise rejected: {}", err));
        }

        resolved_data
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("Promise did not resolve to a lyrics result"))
    }

    pub fn execute_search(
        &self,
        script: &str,
        platform: &str,
        keyword: &str,
        page: usize,
        limit: usize,
    ) -> Result<String> {
        let resolved_data = Arc::new(Mutex::new(None));
        let resolved_err = Arc::new(Mutex::new(None));

        self.arm_deadline();
        let with_res = self.context.as_ref().unwrap().with(|ctx| -> Result<()> {
            let meta = super::loader::parse_metadata(script).ok();
            let mut state_obj = SandboxState::default();
            if let Some(m) = meta {
                state_obj.name = m.name;
                state_obj.description = m.description.unwrap_or_default();
                state_obj.version = m.version.unwrap_or_default();
                state_obj.author = m.author.unwrap_or_default();
                state_obj.homepage = m.homepage.unwrap_or_default();
            }
            state_obj.raw_script = script.to_string();
            let state = Arc::new(Mutex::new(state_obj));

            let res = (|| -> Result<()> {
                super::bridge::inject_lx(&ctx, state.clone())?;

                // Evaluate the source JS script
                ctx.eval::<(), _>(script)
                    .map_err(|e| anyhow!("Failed to evaluate JS script for search: {}", e))?;

                let handler: Function = ctx
                    .globals()
                    .get("__lx_request_handler")
                    .map_err(|_| anyhow!("JS source did not register a request handler"))?;

                // Construct JS musicSearch request arguments
                let js_arg = serde_json::json!({
                    "action": "musicSearch",
                    "source": platform,
                    "info": {
                        "page": page,
                        "limit": limit,
                        "text": keyword
                    }
                });

                // Invoke request handler and get return value
                let js_val_arg = ctx.json_parse(serde_json::to_string(&js_arg)?)?;
                let promise_val: Value = handler.call((js_val_arg,))?;

                // Resolve promise or get string/object
                if let Some(obj) = promise_val.as_object() {
                    if obj.contains_key("then")? {
                        let success_cb = Function::new(ctx.clone(), {
                            let r = resolved_data.clone();
                            move |val: Value| {
                                if let Some(text) = val.as_string().and_then(|s| s.to_string().ok())
                                {
                                    let mut guard = r.lock().unwrap();
                                    *guard = Some(text);
                                }
                            }
                        })?;

                        let fail_cb = Function::new(ctx.clone(), {
                            let e = resolved_err.clone();
                            move |val: Value| {
                                if let Some(text) = val.as_string().and_then(|s| s.to_string().ok())
                                {
                                    let mut guard = e.lock().unwrap();
                                    *guard = Some(text);
                                }
                            }
                        })?;

                        // Use a sophisticated promise wrapper to force JSON stringification on non-strings in JS space
                        let promise_helper: Function = ctx.eval(
                            " (promise, success, fail) => { \
                                promise.then( \
                                    val => { \
                                        if (typeof val === 'string') { \
                                            success(val); \
                                        } else { \
                                            success(JSON.stringify(val)); \
                                        } \
                                    }, \
                                    err => { \
                                        if (typeof err === 'string') { \
                                            fail(err); \
                                        } else if (err && err.message) { \
                                            fail(err.message); \
                                        } else { \
                                            fail(JSON.stringify(err)); \
                                        } \
                                    } \
                                ); \
                            } ",
                        )?;
                        let _: Value = promise_helper.call((obj.clone(), success_cb, fail_cb))?;
                    } else {
                        // Not a promise but an object
                        let helper_fn: Function = ctx.eval(
                            " (val) => typeof val === 'string' ? val : JSON.stringify(val) ",
                        )?;
                        let stringified: String = helper_fn.call((promise_val,))?;
                        let mut guard = resolved_data.lock().unwrap();
                        *guard = Some(stringified);
                    }
                } else if let Some(s) = promise_val.as_string() {
                    let text = s.to_string()?;
                    let mut guard = resolved_data.lock().unwrap();
                    *guard = Some(text);
                } else {
                    let helper_fn: Function =
                        ctx.eval(" (val) => typeof val === 'string' ? val : JSON.stringify(val) ")?;
                    let stringified: String = helper_fn.call((promise_val,))?;
                    let mut guard = resolved_data.lock().unwrap();
                    *guard = Some(stringified);
                };

                // Clear globals handler reference to ensure no leaks
                let _ = ctx
                    .globals()
                    .set("__lx_request_handler", Value::new_null(ctx.clone()));

                Ok(())
            })();

            if let Err(ref e) = res {
                let catch_val = ctx.catch();
                if !catch_val.is_null() && !catch_val.is_undefined() {
                    if let Some(obj) = catch_val.as_object() {
                        if let Ok(msg) = obj.get::<_, String>("message") {
                            return Err(anyhow!(
                                "QuickJS Exception: {} (underlying error: {})",
                                msg,
                                e
                            ));
                        }
                    }
                    return Err(anyhow!(
                        "QuickJS Exception: {:?} (underlying error: {})",
                        catch_val,
                        e
                    ));
                }
            }
            res
        });
        let with_res = self.map_deadline_error(with_res);
        let drained = self.drive_pending_jobs();
        self.disarm_deadline();
        with_res?;
        drained?;

        if let Some(err) = resolved_err.lock().unwrap().clone() {
            return Err(anyhow!("Promise rejected: {}", err));
        }

        resolved_data
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("Promise did not resolve to a search result"))
    }
}

impl Drop for JsSandbox {
    fn drop(&mut self) {
        // 1. Manually drop context first to clear all global objects and JS values
        self.context.take();

        // 2. Trigger garbage collection multiple times to fully clear nested cycle objects
        for _ in 0..10 {
            self.runtime.run_gc();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn infinite_loop_script_times_out() {
        let sandbox = JsSandbox::with_deadline(1).unwrap();
        let started = Instant::now();
        let err = sandbox.execute_init("while (true) {}").unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "timeout did not fire promptly"
        );
        assert!(
            err.to_string().contains("timed out"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn endless_promise_chain_terminates() {
        let script = r#"
            const { EVENT_NAMES, on } = globalThis.lx;
            on(EVENT_NAMES.request, function () {
                function loop() { return Promise.resolve().then(loop); }
                return loop();
            });
        "#;
        let sandbox = JsSandbox::with_deadline(1).unwrap();
        let err = sandbox
            .execute_resolve(script, "kw", "id", "128k", serde_json::json!({}))
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("timed out") || msg.contains("job budget"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn deadline_does_not_break_normal_scripts() {
        let sandbox = JsSandbox::with_deadline(30).unwrap();
        let script = r#"
            const { send } = globalThis.lx;
            send('inited', { status: true, sources: {} });
        "#;
        let val = sandbox.execute_init(script).unwrap();
        assert_eq!(val.pointer("/status").unwrap(), &serde_json::json!(true));

        // A second run on the same sandbox must not be spuriously interrupted
        // (deadline disarmed between executions).
        let script2 = r#"
            globalThis.lx.send('inited', { status: false, sources: {} });
        "#;
        let val2 = sandbox.execute_init(script2).unwrap();
        assert_eq!(val2.pointer("/status").unwrap(), &serde_json::json!(false));
    }
}

#[cfg(test)]
mod resource_limit_tests {
    use super::*;

    #[test]
    fn heap_limit_stops_runaway_allocation() {
        let sandbox = JsSandbox::with_deadline(10).unwrap();
        let err = sandbox
            .execute_init(
                r#"
                var chunks = [];
                while (true) { chunks.push(new ArrayBuffer(1024 * 1024)); }
            "#,
            )
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            !msg.contains("timed out"),
            "hit deadline instead of memory limit: {msg}"
        );
    }
}
