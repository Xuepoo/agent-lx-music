#![allow(
    clippy::manual_div_ceil,
    clippy::match_result_ok,
    clippy::manual_flatten,
    clippy::collapsible_if,
    clippy::manual_repeat_n,
    clippy::collapsible_str_replace
)]
use crate::source::runtime::SandboxState;
use aes::cipher::{BlockEncryptMut, KeyInit, KeyIvInit};
use anyhow::{Result, anyhow};
use cbc::Encryptor;
use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::ZlibEncoder;
use md5::Digest;
use ring::rand::{SecureRandom, SystemRandom};
use rquickjs::function::{MutFn, Rest};
use rquickjs::{Array, ArrayBuffer, Ctx, Function, Object, Value};
use rsa::{BigUint, Pkcs1v15Encrypt, RsaPublicKey};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

pub fn inject_lx<'js>(ctx: &Ctx<'js>, state: Arc<Mutex<SandboxState>>) -> Result<()> {
    // Inject regenerator polyfill to support ES6+ async/await generator scripts
    ctx.eval::<(), _>(REGENERATOR_POLYFILL)?;

    let global = ctx.globals();

    // Inject console
    let console = Object::new(ctx.clone())?;
    let log_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            |_ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<()> {
                let mut s = String::new();
                for arg in args.0 {
                    if let Some(val_str) = arg.as_string().and_then(|s| s.to_string().ok()) {
                        s.push_str(&val_str);
                        s.push(' ');
                    } else if let Ok(json_val) = js_value_to_serde(arg.clone()) {
                        s.push_str(&json_val.to_string());
                        s.push(' ');
                    }
                }
                let has_debug = std::env::args().any(|arg| {
                    arg == "--debug" || arg == "-d" || arg == "--verbose" || arg == "-v"
                });
                if has_debug {
                    eprintln!("[JS] {}", s.trim_end());
                }
                Ok(())
            },
        ),
    )?;
    console.set("log", log_fn.clone())?;
    console.set("info", log_fn.clone())?;
    console.set("warn", log_fn.clone())?;
    console.set("error", log_fn.clone())?;
    console.set("group", log_fn.clone())?;
    console.set("groupEnd", Function::new(ctx.clone(), || {})?)?;
    global.set("console", console)?;

    let lx = Object::new(ctx.clone())?;

    // Event names
    let event_names = Object::new(ctx.clone())?;
    event_names.set("inited", "inited")?;
    event_names.set("request", "request")?;
    event_names.set("updateAlert", "updateAlert")?;
    lx.set("EVENT_NAMES", event_names)?;

    // Basic attributes
    lx.set("env", "cli")?;
    lx.set("version", "2.0.0")?;

    // lx.currentScriptInfo
    let current_script_info = Object::new(ctx.clone())?;
    let (name, desc, ver, author, homepage, raw_script) = {
        let s = state.lock().unwrap();
        (
            s.name.clone(),
            s.description.clone(),
            s.version.clone(),
            s.author.clone(),
            s.homepage.clone(),
            s.raw_script.clone(),
        )
    };
    current_script_info.set("name", name)?;
    current_script_info.set("description", desc)?;
    current_script_info.set("version", ver)?;
    current_script_info.set("author", author)?;
    current_script_info.set("homepage", homepage)?;
    current_script_info.set("rawScript", raw_script)?;
    lx.set("currentScriptInfo", current_script_info)?;

    // lx.send(eventName, data)
    let state_clone_send = state.clone();
    let send_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            move |_ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<()> {
                if args.len() >= 2 {
                    let event_name = args[0]
                        .as_string()
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    let data = args[1].clone();
                    if event_name == "inited" {
                        if let Ok(json_val) = js_value_to_serde(data) {
                            let mut s = state_clone_send.lock().unwrap();
                            s.inited_data = Some(json_val);
                        }
                    }
                }
                Ok(())
            },
        ),
    )?;
    lx.set("send", send_fn)?;

    // lx.on(eventName, handler)
    let on_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<()> {
                if args.len() >= 2 {
                    let event_name = args[0]
                        .as_string()
                        .and_then(|s| s.to_string().ok())
                        .unwrap_or_default();
                    if event_name == "request" {
                        ctx.globals().set("__lx_request_handler", args[1].clone())?;
                    }
                }
                Ok(())
            },
        ),
    )?;
    lx.set("on", on_fn)?;

    // lx.request(url, options, callback)
    let request_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<()> {
                if args.len() < 3 {
                    return Err(throw_err(&ctx, "lx.request requires 3 arguments"));
                }
                let url = args[0]
                    .as_string()
                    .and_then(|s| s.to_string().ok())
                    .ok_or_else(|| anyhow!("url must be a string"))
                    .map_err(|e| throw_err(&ctx, e))?;
                let options = args[1]
                    .as_object()
                    .ok_or_else(|| anyhow!("options must be an object"))
                    .map_err(|e| throw_err(&ctx, e))?;
                let callback =
                    Function::from_value(args[2].clone()).map_err(|e| throw_err(&ctx, e))?;

                let method: String = options.get("method").unwrap_or_else(|_| "GET".to_string());
                let headers_obj: Option<Object> = options.get("headers").ok();
                let body_val: Option<Value> = options.get("body").ok();

                let mut headers = HashMap::new();
                if let Some(h_obj) = headers_obj {
                    for key in h_obj.keys::<String>() {
                        if let Ok(k) = key {
                            if let Ok(v) = h_obj.get::<_, String>(&k) {
                                headers.insert(k, v);
                            }
                        }
                    }
                }

                // Execute reqwest async request using block_in_place
                let result = tokio::task::block_in_place(|| {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(15))
                            .build()?;

                        let mut req = match method.to_uppercase().as_str() {
                            "POST" => client.post(&url),
                            _ => client.get(&url),
                        };

                        for (k, v) in headers {
                            req = req.header(k, v);
                        }

                        if let Some(body) = body_val {
                            if let Some(s) = body.as_string() {
                                if let Ok(text) = s.to_string() {
                                    req = req.body(text);
                                }
                            } else if let Some(obj) = body.as_object() {
                                // If it is a TypedArray/ArrayBuffer, read bytes
                                if let Some(arr_buf) = ArrayBuffer::from_object(obj.clone()) {
                                    let bytes: &[u8] = arr_buf.as_bytes().unwrap_or_default();
                                    req = req.body(bytes.to_vec());
                                }
                            }
                        }

                        let res = req.send().await?;
                        let status = res.status().as_u16();
                        let mut res_headers = HashMap::new();
                        for (k, v) in res.headers() {
                            if let Ok(val) = v.to_str() {
                                res_headers.insert(k.to_string(), val.to_string());
                            }
                        }
                        let raw_bytes = res.bytes().await?.to_vec();
                        Ok::<(u16, HashMap<String, String>, Vec<u8>), reqwest::Error>((
                            status,
                            res_headers,
                            raw_bytes,
                        ))
                    })
                });

                match result {
                    Ok((status, res_headers, raw_bytes)) => {
                        let resp_obj = Object::new(ctx.clone())?;
                        resp_obj.set("statusCode", status)?;
                        resp_obj.set("headers", res_headers)?;

                        // Attempt JSON parsing first, if fails fallback to string
                        let parsed_body = if let Ok(json_str) = std::str::from_utf8(&raw_bytes) {
                            if let Ok(val) = ctx.json_parse(json_str) {
                                val
                            } else {
                                rquickjs::String::from_str(ctx.clone(), json_str)?.into_value()
                            }
                        } else {
                            Value::new_null(ctx.clone())
                        };
                        resp_obj.set("body", parsed_body.clone())?;

                        // raw field as ArrayBuffer
                        let array_buffer = ArrayBuffer::new(ctx.clone(), raw_bytes.clone())?;
                        resp_obj.set("raw", array_buffer)?;

                        let _ = callback.call::<_, ()>((
                            Value::new_null(ctx.clone()),
                            resp_obj,
                            parsed_body,
                        ));
                    }
                    Err(e) => {
                        let err_obj = Object::new(ctx.clone())?;
                        err_obj.set("message", e.to_string())?;
                        let _ = callback.call::<_, ()>((err_obj, Value::new_null(ctx.clone())));
                    }
                }

                Ok(())
            },
        ),
    )?;
    lx.set("request", request_fn)?;

    // lx.utils
    let utils = Object::new(ctx.clone())?;

    // lx.utils.buffer
    let buffer = Object::new(ctx.clone())?;
    let buf_from = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Value<'js>> {
                if args.is_empty() {
                    return Ok(Value::new_null(ctx.clone()));
                }
                let data = args[0].clone();
                let encoding = args
                    .get(1)
                    .and_then(|v| v.as_string())
                    .and_then(|s| s.to_string().ok());

                let bytes = if let Some(s) = data.as_string() {
                    let text = s.to_string().map_err(|e| throw_err(&ctx, e))?;
                    let enc = encoding.unwrap_or_else(|| "utf8".to_string());
                    match enc.as_str() {
                        "hex" => hex::decode(text).unwrap_or_default(),
                        "base64" => {
                            let cleaned =
                                text.replace('\n', "").replace('\r', "").trim().to_string();
                            base64_decode(&cleaned).unwrap_or_default()
                        }
                        _ => text.into_bytes(),
                    }
                } else if let Some(obj) = data.as_object() {
                    if let Some(arr_buf) = ArrayBuffer::from_object(obj.clone()) {
                        arr_buf.as_bytes().unwrap_or_default().to_vec()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };

                let array_buffer = ArrayBuffer::new(ctx.clone(), bytes)?;
                Ok(array_buffer.into_value())
            },
        ),
    )?;
    buffer.set("from", buf_from)?;

    let buf_to_string = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<String> {
                if args.is_empty() {
                    return Ok(String::new());
                }
                let buf = args[0]
                    .as_object()
                    .ok_or_else(|| anyhow!("buffer must be an object"))
                    .map_err(|e| throw_err(&ctx, e))?;
                let encoding = args
                    .get(1)
                    .and_then(|v| v.as_string())
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_default();

                let bytes = if let Some(arr_buf) = ArrayBuffer::from_object(buf.clone()) {
                    arr_buf.as_bytes().unwrap_or_default().to_vec()
                } else {
                    vec![]
                };

                let s = match encoding.as_str() {
                    "hex" => hex::encode(&bytes),
                    "base64" => base64_encode(&bytes),
                    _ => String::from_utf8(bytes).unwrap_or_default(),
                };
                Ok(s)
            },
        ),
    )?;
    buffer.set("bufToString", buf_to_string)?;
    utils.set("buffer", buffer)?;

    // lx.utils.zlib
    // lx.utils.zlib
    let zlib = Object::new(ctx.clone())?;
    let inflate_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Value<'js>> {
                if args.is_empty() {
                    return Ok(Value::new_null(ctx.clone()));
                }
                let buf = args[0]
                    .as_object()
                    .ok_or_else(|| anyhow!("buffer must be an object"))
                    .map_err(|e| throw_err(&ctx, e))?;

                let bytes = if let Some(arr_buf) = ArrayBuffer::from_object(buf.clone()) {
                    arr_buf.as_bytes().unwrap_or_default().to_vec()
                } else {
                    vec![]
                };

                // Attempt Zlib decompression first, fallback to Gzip
                let mut decoded = Vec::new();
                let mut decoder = ZlibDecoder::new(&bytes[..]);
                if decoder.read_to_end(&mut decoded).is_err() {
                    decoded.clear();
                    let mut gz = GzDecoder::new(&bytes[..]);
                    let _ = gz.read_to_end(&mut decoded);
                }

                let array_buffer = ArrayBuffer::new(ctx.clone(), decoded)?;
                Ok(array_buffer.into_value())
            },
        ),
    )?;
    zlib.set("inflate", inflate_fn)?;

    let deflate_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Value<'js>> {
                if args.is_empty() {
                    return Ok(Value::new_null(ctx.clone()));
                }
                let buf = args[0]
                    .as_object()
                    .ok_or_else(|| anyhow!("buffer must be an object"))
                    .map_err(|e| throw_err(&ctx, e))?;

                let bytes = if let Some(arr_buf) = ArrayBuffer::from_object(buf.clone()) {
                    arr_buf.as_bytes().unwrap_or_default().to_vec()
                } else {
                    vec![]
                };

                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(&bytes).map_err(|e| throw_err(&ctx, e))?;
                let compressed = encoder.finish().map_err(|e| throw_err(&ctx, e))?;

                let array_buffer = ArrayBuffer::new(ctx.clone(), compressed)?;
                Ok(array_buffer.into_value())
            },
        ),
    )?;
    zlib.set("deflate", deflate_fn)?;
    utils.set("zlib", zlib)?;

    // lx.utils.crypto
    let crypto = Object::new(ctx.clone())?;

    // md5(string)
    let md5_fn = Function::new(
        ctx.clone(),
        MutFn::new(move |args: Rest<Value<'js>>| -> rquickjs::Result<String> {
            let text = if args.is_empty() {
                String::new()
            } else {
                args[0]
                    .as_string()
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_default()
            };
            let mut hasher = md5::Md5::new();
            hasher.update(text.as_bytes());
            Ok(hex::encode(hasher.finalize()))
        }),
    )?;
    crypto.set("md5", md5_fn)?;

    // randomBytes(size)
    let rand_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Value<'js>> {
                let size = if args.is_empty() {
                    0
                } else {
                    args[0].as_int().unwrap_or(0) as usize
                };
                let mut bytes = vec![0u8; size];
                let rand = SystemRandom::new();
                let _ = rand.fill(&mut bytes);

                let array_buffer = ArrayBuffer::new(ctx.clone(), bytes)?;
                Ok(array_buffer.into_value())
            },
        ),
    )?;
    crypto.set("randomBytes", rand_fn)?;

    // aesEncrypt(buffer, mode, key, iv)
    let aes_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Value<'js>> {
                if args.len() < 4 {
                    return Err(throw_err(&ctx, "aesEncrypt requires 4 arguments"));
                }
                let buffer = args[0].clone();
                let mode = args[1]
                    .as_string()
                    .ok_or_else(|| anyhow!("mode must be a string"))
                    .map_err(|e| throw_err(&ctx, e))?
                    .to_string()
                    .map_err(|e| throw_err(&ctx, e))?;
                let key_val = args[2].clone();
                let iv_val = args[3].clone();

                let obj = buffer
                    .as_object()
                    .ok_or_else(|| anyhow!("buffer must be an object"))
                    .map_err(|e| throw_err(&ctx, e))?;
                let data = if let Some(arr_buf) = ArrayBuffer::from_object(obj.clone()) {
                    arr_buf.as_bytes().unwrap_or_default().to_vec()
                } else {
                    vec![]
                };

                let key = get_bytes_from_val(key_val).map_err(|e| throw_err(&ctx, e))?;
                let iv = get_bytes_from_val(iv_val).map_err(|e| throw_err(&ctx, e))?;

                let encrypted = if mode.contains("cbc") {
                    let mut padded = data.clone();
                    let _ = Encryptor::<aes::Aes128>::new_from_slices(&key, &iv)
                        .map_err(|e| throw_err(&ctx, anyhow!("AES key init failed: {}", e)))?;
                    // Add PKCS7 padding manually or via cipher
                    let len = padded.len();
                    let pad_len = 16 - (len % 16);
                    padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));

                    let mut out = vec![0u8; padded.len()];
                    let mut enc_cbc = Encryptor::<aes::Aes128>::new_from_slices(&key, &iv).unwrap();
                    for chunk_idx in (0..padded.len()).step_by(16) {
                        let mut block =
                            aes::Block::clone_from_slice(&padded[chunk_idx..chunk_idx + 16]);
                        enc_cbc.encrypt_block_b2b_mut(&block.clone(), &mut block);
                        out[chunk_idx..chunk_idx + 16].copy_from_slice(&block);
                    }
                    out
                } else {
                    // ECB (no IV required)
                    let len = data.len();
                    let pad_len = 16 - (len % 16);
                    let mut padded = data.clone();
                    padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));

                    let mut cipher = aes::Aes128::new_from_slice(&key)
                        .map_err(|e| throw_err(&ctx, anyhow!("AES-ECB key init failed: {}", e)))?;

                    let mut out = vec![0u8; padded.len()];
                    for chunk_idx in (0..padded.len()).step_by(16) {
                        let mut block =
                            aes::Block::clone_from_slice(&padded[chunk_idx..chunk_idx + 16]);
                        cipher.encrypt_block_b2b_mut(&block.clone(), &mut block);
                        out[chunk_idx..chunk_idx + 16].copy_from_slice(&block);
                    }
                    out
                };

                let array_buffer = ArrayBuffer::new(ctx.clone(), encrypted)?;
                Ok(array_buffer.into_value())
            },
        ),
    )?;
    crypto.set("aesEncrypt", aes_fn)?;

    // rsaEncrypt(buffer, key)
    let rsa_fn = Function::new(
        ctx.clone(),
        MutFn::new(
            move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Value<'js>> {
                if args.len() < 2 {
                    return Err(throw_err(&ctx, "rsaEncrypt requires 2 arguments"));
                }
                let buffer = args[0].clone();
                let public_key = args[1]
                    .as_string()
                    .ok_or_else(|| anyhow!("public key must be a string"))
                    .map_err(|e| throw_err(&ctx, e))?
                    .to_string()
                    .map_err(|e| throw_err(&ctx, e))?;

                let obj = buffer
                    .as_object()
                    .ok_or_else(|| anyhow!("buffer must be an object"))
                    .map_err(|e| throw_err(&ctx, e))?;
                let data = if let Some(arr_buf) = ArrayBuffer::from_object(obj.clone()) {
                    arr_buf.as_bytes().unwrap_or_default().to_vec()
                } else {
                    vec![]
                };

                let modulus = if public_key.starts_with("-----") {
                    return Err(throw_err(
                        &ctx,
                        "PEM RSA keys not fully parsed in basic bridge yet",
                    ));
                } else {
                    BigUint::from_bytes_be(
                        &hex::decode(public_key).map_err(|e| throw_err(&ctx, e))?,
                    )
                };

                let exponent = BigUint::from(65537u32);
                let pub_key = RsaPublicKey::new(modulus, exponent).map_err(|e| {
                    throw_err(&ctx, anyhow!("Failed to initialize RSA public key: {}", e))
                })?;

                let mut rng = rand::thread_rng();
                let encrypted = pub_key
                    .encrypt(&mut rng, Pkcs1v15Encrypt, &data)
                    .map_err(|e| throw_err(&ctx, anyhow!("RSA encryption failed: {}", e)))?;

                let array_buffer = ArrayBuffer::new(ctx.clone(), encrypted)?;
                Ok(array_buffer.into_value())
            },
        ),
    )?;
    crypto.set("rsaEncrypt", rsa_fn)?;
    utils.set("crypto", crypto)?;

    lx.set("utils", utils)?;
    global.set("lx", lx)?;

    Ok(())
}

fn js_value_to_serde<'js>(val: Value<'js>) -> Result<serde_json::Value> {
    if val.is_null() || val.is_undefined() {
        Ok(serde_json::Value::Null)
    } else if val.is_bool() {
        Ok(serde_json::Value::Bool(val.as_bool().unwrap_or(false)))
    } else if val.is_int() {
        Ok(serde_json::Value::Number(serde_json::Number::from(
            val.as_int().unwrap_or(0),
        )))
    } else if val.is_float() {
        let f = val.as_float().unwrap_or(0.0);
        if let Some(num) = serde_json::Number::from_f64(f) {
            Ok(serde_json::Value::Number(num))
        } else {
            Ok(serde_json::Value::Null)
        }
    } else if let Some(s) = val.as_string() {
        Ok(serde_json::Value::String(s.to_string()?))
    } else if let Some(arr) = Array::from_value(val.clone()).ok() {
        let mut vec = Vec::new();
        for i in 0..arr.len() {
            let item: Value<'js> = arr.get(i)?;
            vec.push(js_value_to_serde(item)?);
        }
        Ok(serde_json::Value::Array(vec))
    } else if let Some(obj) = val.as_object() {
        let mut map = serde_json::Map::new();
        for key in obj.keys::<String>() {
            if let Ok(k) = key {
                if let Ok(v) = obj.get::<_, Value<'js>>(&k) {
                    map.insert(k, js_value_to_serde(v)?);
                }
            }
        }
        Ok(serde_json::Value::Object(map))
    } else {
        Ok(serde_json::Value::Null)
    }
}

fn throw_err<'js>(ctx: &Ctx<'js>, e: impl std::fmt::Display) -> rquickjs::Error {
    let msg = e.to_string();
    if let Ok(js_str) = rquickjs::String::from_str(ctx.clone(), &msg) {
        ctx.throw(js_str.into_value())
    } else {
        rquickjs::Error::Exception
    }
}

fn get_bytes_from_val(val: Value) -> Result<Vec<u8>> {
    if let Some(s) = val.as_string() {
        let text = s.to_string()?;
        Ok(text.into_bytes())
    } else if let Some(obj) = val.as_object() {
        if let Some(arr_buf) = ArrayBuffer::from_object(obj.clone()) {
            Ok(arr_buf.as_bytes().unwrap_or_default().to_vec())
        } else {
            Ok(vec![])
        }
    } else {
        Ok(vec![])
    }
}

// Simple base64 encoder/decoder helpers
fn base64_encode(data: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        match chunk.len() {
            3 => {
                let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
                result.push(CHARSET[((n >> 18) & 63) as usize] as char);
                result.push(CHARSET[((n >> 12) & 63) as usize] as char);
                result.push(CHARSET[((n >> 6) & 63) as usize] as char);
                result.push(CHARSET[(n & 63) as usize] as char);
            }
            2 => {
                let n = ((chunk[0] as u32) << 8) | (chunk[1] as u32);
                result.push(CHARSET[((n >> 10) & 63) as usize] as char);
                result.push(CHARSET[((n >> 4) & 63) as usize] as char);
                result.push(CHARSET[((n << 2) & 63) as usize] as char);
                result.push('=');
            }
            1 => {
                let n = chunk[0] as u32;
                result.push(CHARSET[((n >> 2) & 63) as usize] as char);
                result.push(CHARSET[((n << 4) & 63) as usize] as char);
                result.push('=');
                result.push('=');
            }
            _ => unreachable!(),
        }
    }
    result
}

fn base64_decode(data: &str) -> Result<Vec<u8>> {
    let mut alphabet = [0u8; 256];
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (idx, &ch) in CHARSET.iter().enumerate() {
        alphabet[ch as usize] = idx as u8;
    }

    let cleaned: String = data
        .chars()
        .filter(|&c| c != '=' && !c.is_whitespace())
        .collect();
    let mut result = Vec::new();

    let bytes = cleaned.as_bytes();
    for chunk in bytes.chunks(4) {
        match chunk.len() {
            4 => {
                let n = ((alphabet[chunk[0] as usize] as u32) << 18)
                    | ((alphabet[chunk[1] as usize] as u32) << 12)
                    | ((alphabet[chunk[2] as usize] as u32) << 6)
                    | (alphabet[chunk[3] as usize] as u32);
                result.push(((n >> 16) & 255) as u8);
                result.push(((n >> 8) & 255) as u8);
                result.push((n & 255) as u8);
            }
            3 => {
                let n = ((alphabet[chunk[0] as usize] as u32) << 12)
                    | ((alphabet[chunk[1] as usize] as u32) << 6)
                    | (alphabet[chunk[2] as usize] as u32);
                result.push(((n >> 8) & 255) as u8);
                result.push((n & 255) as u8);
            }
            2 => {
                let n = ((alphabet[chunk[0] as usize] as u32) << 6)
                    | (alphabet[chunk[1] as usize] as u32);
                result.push(((n >> 2) & 255) as u8);
            }
            _ => {}
        }
    }
    Ok(result)
}

const REGENERATOR_POLYFILL: &str = r#"
var _typeof;
globalThis._typeof = function(obj) {
  return typeof obj;
};
var _defineProperty;
globalThis._defineProperty = function(obj, key, value) {
  if (key in obj) {
    Object.defineProperty(obj, key, { value: value, enumerable: true, configurable: true, writable: true });
  } else {
    obj[key] = value;
  }
  return obj;
};
globalThis.global = globalThis;

var DEV_ENABLE;
globalThis.DEV_ENABLE = false;
var UPDATE_ENABLE;
globalThis.UPDATE_ENABLE = false;
var MUSIC_QUALITY;
globalThis.MUSIC_QUALITY = {};
var MUSIC_SOURCE;
globalThis.MUSIC_SOURCE = {};
var API_URL;
globalThis.API_URL = "";
var API_KEY;
globalThis.API_KEY = "";
var _globalThis$lx;
var sm;
var handleBase64Encode;
var handleBase64Decode;
var handleGetMusicUrl;
var handleGetMusicPic;
var handleGetMusicLyric;
var musicSources;
var checkUpdate;
var sha256;
globalThis.EVENT_NAMES = {
  inited: 'inited',
  request: 'request',
  response: 'response'
};

try {
  var _request = undefined;
  Object.defineProperty(globalThis, 'request', {
    get: function() { return _request !== undefined ? _request : (globalThis.lx ? globalThis.lx.request : undefined); },
    set: function(val) { _request = val; },
    configurable: true
  });
} catch(e) {}
try {
  var _send = undefined;
  Object.defineProperty(globalThis, 'send', {
    get: function() { return _send !== undefined ? _send : (globalThis.lx ? globalThis.lx.send : undefined); },
    set: function(val) { _send = val; },
    configurable: true
  });
} catch(e) {}
try {
  var _on = undefined;
  Object.defineProperty(globalThis, 'on', {
    get: function() { return _on !== undefined ? _on : (globalThis.lx ? globalThis.lx.on : undefined); },
    set: function(val) { _on = val; },
    configurable: true
  });
} catch(e) {}
try {
  var _utils = undefined;
  Object.defineProperty(globalThis, 'utils', {
    get: function() { return _utils !== undefined ? _utils : (globalThis.lx ? globalThis.lx.utils : undefined); },
    set: function(val) { _utils = val; },
    configurable: true
  });
} catch(e) {}
try {
  var _env = undefined;
  Object.defineProperty(globalThis, 'env', {
    get: function() { return _env !== undefined ? _env : (globalThis.lx ? globalThis.lx.env : 'desktop'); },
    set: function(val) { _env = val; },
    configurable: true
  });
} catch(e) {}
try {
  var _version = undefined;
  Object.defineProperty(globalThis, 'version', {
    get: function() { return _version !== undefined ? _version : (globalThis.lx ? globalThis.lx.version : '1.0.0'); },
    set: function(val) { _version = val; },
    configurable: true
  });
} catch(e) {}
try {
  var _httpFetch = undefined;
  Object.defineProperty(globalThis, 'httpFetch', {
    get: function() {
      if (_httpFetch !== undefined) return _httpFetch;
      return function(url, options, callback) {
        if (globalThis.lx && globalThis.lx.request) {
          if (typeof callback === 'function') {
            return globalThis.lx.request(url, options, callback);
          }
          var cancelFn = function() {};
          var promise = new Promise(function(resolve, reject) {
            globalThis.lx.request(url, options, function(err, resp, body) {
              if (err) {
                reject(err);
              } else {
                resolve(resp);
              }
            });
          });
          return {
            promise: promise,
            cancel: cancelFn
          };
        }
        throw new Error("lx.request is not available");
      };
    },
    set: function(val) { _httpFetch = val; },
    configurable: true
  });
} catch(e) {}

var _regenerator;
var _regeneratorDefine2;
var _regeneratorRuntime;
var regeneratorRuntime;
var asyncGeneratorStep;
var _asyncToGenerator;
var runtime=function(t){"use strict";var r,e=Object.prototype,n=e.hasOwnProperty,o=Object.defineProperty||function(t,r,e){t[r]=e.value},i="function"==typeof Symbol?Symbol:{},a=i.iterator||"@@iterator",c=i.asyncIterator||"@@asyncIterator",u=i.toStringTag||"@@toStringTag";function h(t,r,e){return Object.defineProperty(t,r,{value:e,enumerable:!0,configurable:!0,writable:!0}),t[r]}try{h({},"")}catch(t){h=function(t,r,e){return t[r]=e}}function l(t,r,e,n){var i=r&&r.prototype instanceof d?r:d,a=Object.create(i.prototype),c=new T(n||[]);return o(a,"_invoke",{value:O(t,e,c)}),a}function f(t,r,e){try{return{type:"normal",arg:t.call(r,e)}}catch(t){return{type:"throw",arg:t}}}t.wrap=l;var s="suspendedStart",p="suspendedYield",y="executing",v="completed",g={};function d(){}function m(){}function w(){}var b={};h(b,a,(function(){return this}));var L=Object.getPrototypeOf,x=L&&L(L(P([])));x&&x!==e&&n.call(x,a)&&(b=x);var E=w.prototype=d.prototype=Object.create(b);function j(t){["next","throw","return"].forEach((function(r){h(t,r,(function(t){return this._invoke(r,t)}))}))}function _(t,r){function e(o,i,a,c){var u=f(t[o],t,i);if("throw"!==u.type){var h=u.arg,l=h.value;return l&&"object"==typeof l&&n.call(l,"__await")?r.resolve(l.__await).then((function(t){e("next",t,a,c)}),(function(t){e("throw",t,a,c)})):r.resolve(l).then((function(t){h.value=t,a(h)}),(function(t){return e("throw",t,a,c)}))}c(u.arg)}var i;o(this,"_invoke",{value:function(t,n){function o(){return new r((function(r,o){e(t,n,r,o)}))}return i=i?i.then(o,o):o()}})}function O(t,e,n){var o=s;return function(i,a){if(o===y)throw new Error("Generator is already running");if(o===v){if("throw"===i)throw a;return{value:r,done:!0}}for(n.method=i,n.arg=a;;){var c=n.delegate;if(c){var u=k(c,n);if(u){if(u===g)continue;return u}}if("next"===n.method)n.sent=n._sent=n.arg;else if("throw"===n.method){if(o===s)throw o=v,n.arg;n.dispatchException(n.arg)}else"return"===n.method&&n.abrupt("return",n.arg);o=y;var h=f(t,e,n);if("normal"===h.type){if(o=n.done?v:p,h.arg===g)continue;return{value:h.arg,done:n.done}}"throw"===h.type&&(o=v,n.method="throw",n.arg=h.arg)}}}function k(t,e){var n=e.method,o=t.iterator[n];if(o===r)return e.delegate=null,"throw"===n&&t.iterator.return&&(e.method="return",e.arg=r,k(t,e),"throw"===e.method)||"return"!==n&&(e.method="throw",e.arg=new TypeError("The iterator does not provide a '"+n+"' method")),g;var i=f(o,t.iterator,e.arg);if("throw"===i.type)return e.method="throw",e.arg=i.arg,e.delegate=null,g;var a=i.arg;return a?a.done?(e[t.resultName]=a.value,e.next=t.nextLoc,"return"!==e.method&&(e.method="next",e.arg=r),e.delegate=null,g):a:(e.method="throw",e.arg=new TypeError("iterator result is not an object"),e.delegate=null,g)}function G(t){var r={tryLoc:t[0]};1 in t&&(r.catchLoc=t[1]),2 in t&&(r.finallyLoc=t[2],r.afterLoc=t[3]),this.tryEntries.push(r)}function N(t){var r=t.completion||{};r.type="normal",delete r.arg,t.completion=r}function T(t){this.tryEntries=[{tryLoc:"root"}],t.forEach(G,this),this.reset(!0)}function P(t){if(null!=t){var e=t[a];if(e)return e.call(t);if("function"==typeof t.next)return t;if(!isNaN(t.length)){var o=-1,i=function e(){for(;++o<t.length;)if(n.call(t,o))return e.value=t[o],e.done=!1,e;return e.value=r,e.done=!0,e};return i.next=i}}throw new TypeError(typeof t+" is not iterable")}return m.prototype=w,o(E,"constructor",{value:w,configurable:!0}),o(w,"constructor",{value:m,configurable:!0}),m.displayName=h(w,u,"GeneratorFunction"),t.isGeneratorFunction=function(t){var r="function"==typeof t&&t.constructor;return!!r&&(r===m||"GeneratorFunction"===(r.displayName||r.name))},t.mark=function(t){return Object.setPrototypeOf?Object.setPrototypeOf(t,w):(t.__proto__=w,h(t,u,"GeneratorFunction")),t.prototype=Object.create(E),t},t.awrap=function(t){return{__await:t}},j(_.prototype),h(_.prototype,c,(function(){return this})),t.AsyncIterator=_,t.async=function(r,e,n,o,i){void 0===i&&(i=Promise);var a=new _(l(r,e,n,o),i);return t.isGeneratorFunction(e)?a:a.next().then((function(t){return t.done?t.value:a.next()}))},j(E),h(E,u,"Generator"),h(E,a,(function(){return this})),h(E,"toString",(function(){return"[object Generator]"})),t.keys=function(t){var r=Object(t),e=[];for(var n in r)e.push(n);return e.reverse(),function t(){for(;e.length;){var n=e.pop();if(n in r)return t.value=n,t.done=!1,t}return t.done=!0,t}},t.values=P,T.prototype={constructor:T,reset:function(t){if(this.prev=0,this.next=0,this.sent=this._sent=r,this.done=!1,this.delegate=null,this.method="next",this.arg=r,this.tryEntries.forEach(N),!t)for(var e in this)"t"===e.charAt(0)&&n.call(this,e)&&!isNaN(+e.slice(1))&&(this[e]=r)},stop:function(){this.done=!0;var t=this.tryEntries[0].completion;if("throw"===t.type)throw t.arg;return this.rval},dispatchException:function(t){if(this.done)throw t;var e=this;function o(n,o){return c.type="throw",c.arg=t,e.next=n,o&&(e.method="next",e.arg=r),!!o}for(var i=this.tryEntries.length-1;i>=0;--i){var a=this.tryEntries[i],c=a.completion;if("root"===a.tryLoc)return o("end");if(a.tryLoc<=this.prev){var u=n.call(a,"catchLoc"),h=n.call(a,"finallyLoc");if(u&&h){if(this.prev<a.catchLoc)return o(a.catchLoc,!0);if(this.prev<a.finallyLoc)return o(a.finallyLoc)}else if(u){if(this.prev<a.catchLoc)return o(a.catchLoc,!0)}else{if(!h)throw new Error("try statement without catch or finally");if(this.prev<a.finallyLoc)return o(a.finallyLoc)}}}},abrupt:function(t,r){for(var e=this.tryEntries.length-1;e>=0;--e){var o=this.tryEntries[e];if(o.tryLoc<=this.prev&&n.call(o,"finallyLoc")&&this.prev<o.finallyLoc){var i=o;break}}i&&("break"===t||"continue"===t)&&i.tryLoc<=r&&r<=i.finallyLoc&&(i=null);var a=i?i.completion:{};return a.type=t,a.arg=r,i?(this.method="next",this.next=i.finallyLoc,g):this.complete(a)},complete:function(t,r){if("throw"===t.type)throw t.arg;return"break"===t.type||"continue"===t.type?this.next=t.arg:"return"===t.type?(this.rval=this.arg=t.arg,this.method="return",this.next="end"):"normal"===t.type&&r&&(this.next=r),g},finish:function(t){for(var r=this.tryEntries.length-1;r>=0;--r){var e=this.tryEntries[r];if(e.finallyLoc===t)return this.complete(e.completion,e.afterLoc),N(e),g}},catch:function(t){for(var r=this.tryEntries.length-1;r>=0;--r){var e=this.tryEntries[r];if(e.tryLoc===t){var n=e.completion;if("throw"===n.type){var o=n.arg;N(e)}return o}}throw new Error("illegal catch attempt")},delegateYield:function(t,e,n){return this.delegate={iterator:P(t),resultName:e,nextLoc:n},"next"===this.method&&(this.arg=r),g}},t}("object"==typeof module?module.exports:{});
globalThis.regeneratorRuntime = runtime;
globalThis._regenerator = { default: runtime };
globalThis._regeneratorRuntime = runtime;
globalThis._regeneratorDefine2 = function(r) { return r; };

globalThis.asyncGeneratorStep = function(gen, resolve, reject, _next, _throw, key, arg) {
  try {
    var info = gen[key](arg);
    var value = info.value;
  } catch (error) {
    reject(error);
    return;
  }
  if (info.done) {
    resolve(value);
  } else {
    Promise.resolve(value).then(_next, _throw);
  }
};

globalThis._asyncToGenerator = function(fn) {
  return function () {
    var self = this, args = arguments;
    return new Promise(function (resolve, reject) {
      var gen = fn.apply(self, args);
      function _next(value) {
        globalThis.asyncGeneratorStep(gen, resolve, reject, _next, _throw, "next", value);
      }
      function _throw(err) {
        globalThis.asyncGeneratorStep(gen, resolve, reject, _next, _throw, "throw", err);
      }
      _next(undefined);
    });
  };
};
"#;
