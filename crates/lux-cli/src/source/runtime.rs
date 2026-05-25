#![allow(clippy::arc_with_non_send_sync, clippy::collapsible_if)]
use anyhow::{Result, anyhow};
use rquickjs::{Context, Function, Runtime, Value};
use std::sync::{Arc, Mutex};

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
}

impl JsSandbox {
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()?;
        let context = Context::full(&runtime)?;
        Ok(Self {
            context: Some(context),
            runtime,
        })
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

        self.context.as_ref().unwrap().with(|ctx| -> Result<()> {
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
        })?;

        // Drive the microtasks event loop OUTSIDE of context.with(...)!
        while self.runtime.execute_pending_job().unwrap_or(false) {}

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

        self.context.as_ref().unwrap().with(|ctx| -> Result<()> {
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
        })?;

        // Drive potential microtasks OUTSIDE of context.with(...)!
        while self.runtime.execute_pending_job().unwrap_or(false) {}

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

        self.context.as_ref().unwrap().with(|ctx| -> Result<()> {
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
        })?;

        // Drive the microtasks event loop OUTSIDE of context.with(...)!
        while self.runtime.execute_pending_job().unwrap_or(false) {}

        if let Some(err) = resolved_err.lock().unwrap().clone() {
            return Err(anyhow!("Promise rejected: {}", err));
        }

        resolved_data
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("Promise did not resolve to a lyrics result"))
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
