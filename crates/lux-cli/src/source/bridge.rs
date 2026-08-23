#![allow(
    deprecated,
    clippy::manual_div_ceil,
    clippy::match_result_ok,
    clippy::manual_flatten,
    clippy::collapsible_if,
    clippy::manual_repeat_n,
    clippy::collapsible_str_replace
)]
use crate::source::runtime::SandboxState;
use aes::cipher::{BlockCipherEncrypt, BlockModeEncrypt, KeyInit, KeyIvInit};
use anyhow::{Result, anyhow};
use cbc::Encryptor;
use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::ZlibEncoder;
use md5::Digest;
use ring::rand::{SecureRandom, SystemRandom};
use rquickjs::function::{MutFn, Rest};
use rquickjs::{Array, ArrayBuffer, Ctx, Exception, Function, Object, Value};
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, Pkcs1v15Encrypt, RsaPublicKey};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// Maximum size accepted by `lx.utils.crypto.randomBytes`.
const MAX_RANDOM_BYTES: usize = 64 * 1024 * 1024;
/// Maximum decompressed output accepted from `lx.utils.zlib.inflate`.
const MAX_INFLATE_OUTPUT: usize = 64 * 1024 * 1024;

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

                // Attempt Zlib decompression first, fallback to Gzip. Both
                // paths read at most MAX_INFLATE_OUTPUT + 1 bytes so a bomb
                // cannot make us allocate without bound.
                let mut decoded = Vec::new();
                let mut decoder = ZlibDecoder::new(&bytes[..]);
                let zlib_ok = decoder
                    .by_ref()
                    .take(MAX_INFLATE_OUTPUT as u64 + 1)
                    .read_to_end(&mut decoded)
                    .is_ok();
                if !zlib_ok {
                    decoded.clear();
                    let mut gz = GzDecoder::new(&bytes[..]);
                    let _ = gz
                        .by_ref()
                        .take(MAX_INFLATE_OUTPUT as u64 + 1)
                        .read_to_end(&mut decoded);
                }
                if decoded.len() > MAX_INFLATE_OUTPUT {
                    return Err(Exception::throw_range(
                        &ctx,
                        &format!("inflate output exceeds maximum of {MAX_INFLATE_OUTPUT} bytes"),
                    ));
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
                if size > MAX_RANDOM_BYTES {
                    return Err(Exception::throw_range(
                        &ctx,
                        &format!("randomBytes size exceeds maximum of {MAX_RANDOM_BYTES} bytes"),
                    ));
                }
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
                        let mut block = *aes::Block::from_slice(&padded[chunk_idx..chunk_idx + 16]);
                        enc_cbc.encrypt_block_b2b(&block.clone(), &mut block);
                        out[chunk_idx..chunk_idx + 16].copy_from_slice(&block);
                    }
                    out
                } else {
                    // ECB (no IV required)
                    let len = data.len();
                    let pad_len = 16 - (len % 16);
                    let mut padded = data.clone();
                    padded.extend(std::iter::repeat(pad_len as u8).take(pad_len));

                    let cipher = aes::Aes128::new_from_slice(&key)
                        .map_err(|e| throw_err(&ctx, anyhow!("AES-ECB key init failed: {}", e)))?;

                    let mut out = vec![0u8; padded.len()];
                    for chunk_idx in (0..padded.len()).step_by(16) {
                        let mut block = *aes::Block::from_slice(&padded[chunk_idx..chunk_idx + 16]);
                        cipher.encrypt_block_b2b(&block.clone(), &mut block);
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

                if public_key.starts_with("-----BEGIN") {
                    // Upstream lx-music-desktop contract (preload.js): PEM
                    // public key + RSA_NO_PADDING over the input zero-left-
                    // padded to the modulus size. Raw RSA is deterministic:
                    // c = m^e mod n.
                    let pub_key = RsaPublicKey::from_public_key_pem(&public_key)
                        .or_else(|_| RsaPublicKey::from_pkcs1_pem(&public_key))
                        .map_err(|e| {
                            throw_err(&ctx, anyhow!("Failed to parse PEM RSA public key: {}", e))
                        })?;
                    let n = pub_key.n().clone();
                    let block_size = (n.bits() + 7) / 8;
                    if data.len() > block_size {
                        return Err(Exception::throw_range(
                            &ctx,
                            &format!(
                                "rsaEncrypt input exceeds RSA block size of {block_size} bytes"
                            ),
                        ));
                    }
                    let mut padded = vec![0u8; block_size - data.len()];
                    padded.extend_from_slice(&data);
                    let c = BigUint::from_bytes_be(&padded).modpow(pub_key.e(), &n);
                    let c_bytes = c.to_bytes_be();
                    let mut out = vec![0u8; block_size - c_bytes.len()];
                    out.extend_from_slice(&c_bytes);

                    let array_buffer = ArrayBuffer::new(ctx.clone(), out)?;
                    Ok(array_buffer.into_value())
                } else {
                    // Legacy alx path: hex-encoded modulus with e = 65537,
                    // kept on PKCS#1 v1.5 for backwards compatibility with
                    // existing scripts written against this bridge.
                    let modulus = BigUint::from_bytes_be(
                        &hex::decode(public_key).map_err(|e| throw_err(&ctx, e))?,
                    );

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
                }
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
                // 3 chars = 18-bit value -> 2 bytes (16 bits); low 2 bits are
                // canonical padding. byte0 = top 8 bits, byte1 = next 8 bits.
                let n = ((alphabet[chunk[0] as usize] as u32) << 12)
                    | ((alphabet[chunk[1] as usize] as u32) << 6)
                    | (alphabet[chunk[2] as usize] as u32);
                result.push(((n >> 10) & 255) as u8);
                result.push(((n >> 2) & 255) as u8);
            }
            2 => {
                // 2 chars = 12-bit value -> 1 byte; low 4 bits are padding.
                let n = ((alphabet[chunk[0] as usize] as u32) << 6)
                    | (alphabet[chunk[1] as usize] as u32);
                result.push(((n >> 4) & 255) as u8);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decode_padded_final_chunk_three_chars() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("YWI=").unwrap(), b"ab");
    }

    #[test]
    fn base64_decode_padded_single_byte() {
        assert_eq!(base64_decode("YQ==").unwrap(), b"a");
    }

    #[test]
    fn base64_decode_unpadded_input_still_works() {
        assert_eq!(base64_decode("aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64_decode("YQ").unwrap(), b"a");
        assert_eq!(base64_decode("").unwrap(), b"");
    }

    #[test]
    fn base64_encode_known_vectors_standard_alphabet() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn base64_roundtrip_random_lengths() {
        let mut seed: u64 = 0x1234_5678_9abc_def0;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seed >> 11
        };
        for len in 0usize..64 {
            let buf: Vec<u8> = (0..len).map(|_| (next() & 0xff) as u8).collect();
            let enc = base64_encode(&buf);
            assert_eq!(
                base64_decode(&enc).unwrap(),
                buf,
                "round-trip failed for len {len}"
            );
        }
    }

    fn hex_of(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Evaluate a JS expression with the full `lx` bridge injected; the
    /// expression must evaluate to a string.
    fn eval_with_lx(script: &str) -> Result<String> {
        let sandbox = crate::source::runtime::JsSandbox::new()?;
        let context = sandbox.context.as_ref().unwrap();
        context.with(|ctx| -> Result<String> {
            inject_lx(&ctx, Arc::new(Mutex::new(SandboxState::default())))?;
            let value: Value = match ctx.eval(script) {
                Ok(v) => v,
                Err(e) => {
                    let detail = ctx
                        .catch()
                        .as_object()
                        .and_then(|o| o.get::<_, String>("message").ok())
                        .unwrap_or_else(|| "<no message>".into());
                    return Err(anyhow!("JS eval failed: {e} ({detail})"));
                }
            };
            let s = value
                .as_string()
                .ok_or_else(|| anyhow!("script did not return a string"))?
                .to_string()?;
            Ok(s)
        })
    }

    #[test]
    fn random_bytes_returns_requested_length() {
        let out = eval_with_lx(
            "(function(){ var b = lx.utils.crypto.randomBytes(1024); return 'len:' + b.byteLength; })()",
        )
        .unwrap();
        assert_eq!(out, "len:1024");
    }

    #[test]
    fn random_bytes_over_cap_throws_range_error() {
        let out = eval_with_lx(
            "(function(){ try { lx.utils.crypto.randomBytes(64 * 1024 * 1024 + 1); return 'no-error'; } catch (e) { return e.name + ':' + e.message; } })()",
        )
        .unwrap();
        assert!(out.starts_with("RangeError:"), "got: {out}");
        assert!(out.contains("exceeds"), "got: {out}");
    }

    #[test]
    fn inflate_small_payload_roundtrip() {
        use std::io::Write as _;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"hello world").unwrap();
        let compressed = encoder.finish().unwrap();
        let out = eval_with_lx(&format!(
            "(function(){{ var b = lx.utils.zlib.inflate(lx.utils.buffer.from('{}', 'hex')); return lx.utils.buffer.bufToString(b); }})()",
            hex_of(&compressed)
        ))
        .unwrap();
        assert_eq!(out, "hello world");
    }

    #[test]
    fn inflate_over_cap_errors_instead_of_unbounded_read() {
        use std::io::Write as _;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&vec![0u8; 64 * 1024 * 1024 + 1]).unwrap();
        let compressed = encoder.finish().unwrap();
        let out = eval_with_lx(&format!(
            "(function(){{ try {{ lx.utils.zlib.inflate(lx.utils.buffer.from('{}', 'hex')); return 'no-error'; }} catch (e) {{ return e.name + ':' + e.message; }} }})()",
            hex_of(&compressed)
        ))
        .unwrap();
        assert!(out.starts_with("RangeError:"), "got: {out}");
        assert!(out.contains("exceeds"), "got: {out}");
    }

    // Fixed 1024-bit RSA test keypair (test-only material, never used in
    // production; embedded so tests are fully deterministic).
    const RSA_PUB_PEM: &str = "-----BEGIN PUBLIC KEY-----\nMIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDtoJYSBwozYUP3c90ciX+76+UW\ncmqCPEFSiL+QhKVa+PXqXntyVS5YtkGajiaJdap2iEGt91dgP2Zag30I3aTCssGR\n5BLWL6JK1wFJR3uVUzZr4VpkylnMUI9tomP6k2guxm19s1WfYUH70kmwTUKqk5dN\nIJCNb84gKtShV0MzVQIDAQAB\n-----END PUBLIC KEY-----";
    const RSA_PRIV_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMIICeAIBADANBgkqhkiG9w0BAQEFAASCAmIwggJeAgEAAoGBAO2glhIHCjNhQ/dz\n3RyJf7vr5RZyaoI8QVKIv5CEpVr49epee3JVLli2QZqOJol1qnaIQa33V2A/ZlqD\nfQjdpMKywZHkEtYvokrXAUlHe5VTNmvhWmTKWcxQj22iY/qTaC7GbX2zVZ9hQfvS\nSbBNQqqTl00gkI1vziAq1KFXQzNVAgMBAAECgYEAmRro1oC2r9gxpJSAmMx3AqvB\nFS9vNK6CIB1/4Cu3JuBWAkYSH96GWB5GMsD4T4UC6hBs0RwWrirrVdJ2k2nLp3EC\nk0+nf1IPou7YEKooxGV/QgjO4HKrLROBozWlITYYC938ao3q0s0dHHm7mXcBYOm+\nny/Z8ubOlXxoLEbWocECQQD73hfb+iTxbBW7kAHib5FdXpeFGMAAU0b7ZubR8ybA\n8AeAD/5N1Aw4PgPfYKuRwM0+rYQO9BVAK10GswXHq48pAkEA8YauKzGwTMfNGy6L\nanhpYuMrYfxzFBayiyKwpo7hdKL9+ePBzeuX8BzBSIB0kmjmM1deG/itYxL6wOmf\nDxiETQJBANWWCXWqMxnoJqXgATkck5EyXhuoWWntNQyMvsDcCckjw7h915H4eERZ\nkr8jI1t+vI6iZpKnuj2oiELeHdCtU8ECQASGjYTprXAC3mj/+kTIdNERiKKRZGaf\n9kB9Keo1CyxwUWn5RoxhObuaDlUZcxW7OXUE0hKcGkOc+23Z8s0JnJECQQDdAjCW\nyIeIqH+YKGL/qaWEt0snXp2wjfbaBq4xbQkko2VD+NNa0R8r3KzCL3QmPrI/MwZJ\nxsTtFjVqEBgTuQm0\n-----END PRIVATE KEY-----";
    const RSA_N_HEX: &str = "eda09612070a336143f773dd1c897fbbebe516726a823c415288bf9084a55af8f5ea5e7b72552e58b6419a8e268975aa768841adf757603f665a837d08dda4c2b2c191e412d62fa24ad70149477b9553366be15a64ca59cc508f6da263fa93682ec66d7db3559f6141fbd249b04d42aa93974d20908d6fce202ad4a157433355";
    const RSA_D_HEX: &str = "991ae8d680b6afd831a4948098cc7702abc1152f6f34ae82201d7fe02bb726e0560246121fde86581e4632c0f84f8502ea106cd11c16ae2aeb55d2769369cba77102934fa77f520fa2eed810aa28c4657f4208cee072ab2d1381a335a52136180bddfc6a8dead2cd1d1c79bb99770160e9be9f2fd9f2e6ce957c682c46d6a1c1";

    #[test]
    fn rsa_encrypt_pem_zero_left_pads_and_recovers_plaintext() {
        let script = format!(
            "(function(){{ try {{ \
                var ct = lx.utils.crypto.rsaEncrypt(lx.utils.buffer.from('68656c6c6f', 'hex'), '{}'); \
                return lx.utils.buffer.bufToString(ct, 'hex'); \
            }} catch (e) {{ return 'err:' + e.name + ':' + e.message; }} }})()",
            RSA_PUB_PEM.replace('\n', "\\n")
        );
        let out = eval_with_lx(&script).unwrap();
        assert!(!out.starts_with("err:"), "{out}");
        let ct = hex::decode(out.trim()).unwrap();
        assert_eq!(ct.len(), 128);

        // Deterministic raw RSA: c = (0x00..00 || "hello")^e mod n.
        let n = BigUint::from_bytes_be(&hex::decode(RSA_N_HEX).unwrap());
        let e = BigUint::from(65537u32);
        let mut padded = vec![0u8; 128 - 5];
        padded.extend_from_slice(b"hello");
        let expected = BigUint::from_bytes_be(&padded).modpow(&e, &n).to_bytes_be();
        let mut expected_full = vec![0u8; 128 - expected.len()];
        expected_full.extend_from_slice(&expected);
        assert_eq!(ct, expected_full);

        // Decrypt with the private exponent: recovers the zero-padded block.
        let d = BigUint::from_bytes_be(&hex::decode(RSA_D_HEX).unwrap());
        let recovered = BigUint::from_bytes_be(&ct).modpow(&d, &n).to_bytes_be();
        let mut recovered_full = vec![0u8; 128 - recovered.len()];
        recovered_full.extend_from_slice(&recovered);
        assert_eq!(recovered_full, padded);
    }

    #[test]
    fn rsa_encrypt_pem_rejects_input_larger_than_block() {
        let long_hex = "41".repeat(129);
        let template = "(function(){ try { lx.utils.crypto.rsaEncrypt(lx.utils.buffer.from('@@HEX@@', 'hex'), '@@KEY@@'); return 'no-error'; } catch (e) { return e.name; } })()";
        let script = template
            .replace("@@HEX@@", &long_hex)
            .replace("@@KEY@@", &RSA_PUB_PEM.replace('\n', "\\n"));
        let out = eval_with_lx(&script).unwrap();
        assert!(out.ends_with("RangeError"), "got: {out}");
    }

    #[test]
    fn rsa_encrypt_legacy_hex_modulus_keeps_pkcs1_v15() {
        let modulus_hex = RSA_N_HEX.to_string();
        let script = format!(
            "(function(){{ try {{ \
                var ct = lx.utils.crypto.rsaEncrypt(lx.utils.buffer.from('68656c6c6f', 'hex'), '{}'); \
                return lx.utils.buffer.bufToString(ct, 'hex'); \
            }} catch (e) {{ return 'err:' + e.name + ':' + e.message; }} }})()",
            modulus_hex
        );
        let out = eval_with_lx(&script).unwrap();
        assert!(!out.starts_with("err:"), "{out}");
        let ct = hex::decode(out.trim()).unwrap();
        assert_eq!(ct.len(), 128);

        // PKCS#1 v1.5 unpad via raw private op: 0x00 0x02 <pad> 0x00 <msg>.
        let n = BigUint::from_bytes_be(&hex::decode(RSA_N_HEX).unwrap());
        let d = BigUint::from_bytes_be(&hex::decode(RSA_D_HEX).unwrap());
        let m = BigUint::from_bytes_be(&ct).modpow(&d, &n).to_bytes_be();
        let mut block = vec![0u8; 128 - m.len()];
        block.extend_from_slice(&m);
        assert_eq!(block[0], 0);
        assert_eq!(block[1], 2);
        let sep = (2..block.len())
            .find(|&i| block[i] == 0)
            .expect("v1.5 separator");
        assert_eq!(&block[sep + 1..], b"hello");
    }
}
