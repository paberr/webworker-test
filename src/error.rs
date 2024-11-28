use js_sys::wasm_bindgen::JsValue;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("WebWorker capacity reached")]
pub struct Full;
