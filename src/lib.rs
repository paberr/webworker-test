use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::de;
use serde::{de::Visitor, Deserialize, Serialize};
use std::io::Cursor;
use std::rc::Rc;
use std::{cell::RefCell, fmt};
use wasm_bindgen::prelude::*;
pub use wasm_bindgen_rayon::init_thread_pool;
use web_sys::{
    console, HtmlElement, HtmlInputElement, MessageEvent, Worker, WorkerOptions, WorkerType,
};

use nimiq_bls::{CompressedPublicKey, PublicKey, SecretKey};
use nimiq_utils::key_rng::SecureGenerate;

fn setup_validators(num: usize) -> CompressedKeys {
    console::log_1(&"Create validator keys".into());

    let mut validators = vec![];
    for _ in 0..num {
        // Make sure we only have the compressed key.
        let bls_secret_key = SecretKey::generate_default_csprng();
        let bls_public_key = PublicKey::from_secret(&bls_secret_key);
        validators.push(bls_public_key.compress());
    }

    CompressedKeys(validators)
}

struct UncompressedPublicKey(PublicKey);

impl Serialize for UncompressedPublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let size = CanonicalSerialize::uncompressed_size(&self.0.public_key);
        let mut bytes = vec![0; size];
        CanonicalSerialize::serialize_uncompressed(&self.0.public_key, &mut bytes[..]).unwrap();
        serializer.serialize_bytes(&bytes)
    }
}

impl<'de> Deserialize<'de> for UncompressedPublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PkVisitor;

        impl<'de> Visitor<'de> for PkVisitor {
            type Value = UncompressedPublicKey;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a serialized representation of UncompressedPublicKey")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let reader = Cursor::new(v);
                Ok(UncompressedPublicKey(PublicKey::new(
                    CanonicalDeserialize::deserialize_uncompressed_unchecked(reader).unwrap(),
                )))
            }
        }

        deserializer.deserialize_bytes(PkVisitor)
    }
}

#[derive(Serialize, Deserialize)]
struct CompressedKeys(Vec<CompressedPublicKey>);
#[derive(Serialize, Deserialize)]
struct UncompressedKeys(Vec<UncompressedPublicKey>);

impl From<CompressedKeys> for UncompressedKeys {
    fn from(compressed: CompressedKeys) -> Self {
        UncompressedKeys(
            compressed
                .0
                .into_iter()
                .map(|key| UncompressedPublicKey(key.uncompress().unwrap()))
                .collect(),
        )
    }
}

#[wasm_bindgen]
pub fn uncompress_validators(compressed: JsValue) -> JsValue {
    console::log_1(&"Deserialize value".into());
    let compressed: CompressedKeys = serde_wasm_bindgen::from_value(compressed).unwrap();

    // Do the work.
    console::log_1(&"Uncompress keys".into());
    let uncompressed = UncompressedKeys::from(compressed);

    console::log_1(&"Serialize value".into());
    serde_wasm_bindgen::to_value(&uncompressed).unwrap()
}

/// Run entry point for the main thread.
#[wasm_bindgen]
pub fn startup() {
    // Here, we create our worker. In a larger app, multiple callbacks should be
    // able to interact with the code in the worker. Therefore, we wrap it in
    // `Rc<RefCell>` following the interior mutability pattern. Here, it would
    // not be needed but we include the wrapping anyway as example.
    let opts = WorkerOptions::new();
    opts.set_type(WorkerType::Module);
    let worker_handle = Rc::new(RefCell::new(
        Worker::new_with_options("./worker.js", &opts).unwrap(),
    ));
    console::log_1(&"Created a new worker from within Wasm".into());

    // Pass the worker to the function which sets up the `oninput` callback.
    setup_input_oninput_callback(worker_handle);
}

fn setup_input_oninput_callback(worker: Rc<RefCell<web_sys::Worker>>) {
    let document = web_sys::window().unwrap().document().unwrap();

    // If our `onmessage` callback should stay valid after exiting from the
    // `oninput` closure scope, we need to either forget it (so it is not
    // destroyed) or store it somewhere. To avoid leaking memory every time we
    // want to receive a response from the worker, we move a handle into the
    // `oninput` closure to which we will always attach the last `onmessage`
    // callback. The initial value will not be used and we silence the warning.
    #[allow(unused_assignments)]
    let mut persistent_callback_handle = get_on_msg_callback();

    let callback = Closure::new(move || {
        console::log_1(&"oninput callback triggered".into());
        let document = web_sys::window().unwrap().document().unwrap();

        let input_field = document
            .get_element_by_id("num_keys")
            .expect("#num_keys should exist");
        let input_field = input_field
            .dyn_ref::<HtmlInputElement>()
            .expect("#num_keys should be a HtmlInputElement");

        // If the value in the field can be parsed to a `usize`, prepare the compressed keys.
        let num_keys = match input_field.value().parse::<usize>() {
            Ok(number) => number,
            Err(_) => {
                console::log_1(&"Error parsing value".into());
                return;
            }
        };

        let input_field = document
            .get_element_by_id("webworker")
            .expect("#webworker should exist");
        let input_field = input_field
            .dyn_ref::<HtmlInputElement>()
            .expect("#webworker should be a HtmlInputElement");

        let in_webworker = input_field.checked();

        let keys = setup_validators(num_keys);

        // Access worker behind shared handle, following the interior
        // mutability pattern.
        if in_webworker {
            console::log_1(&"postMessage to worker".into());
            let worker_handle = &*worker.borrow();
            let _ = worker_handle.post_message(&serde_wasm_bindgen::to_value(&keys).unwrap());
            persistent_callback_handle = get_on_msg_callback();

            // Since the worker returns the message asynchronously, we
            // attach a callback to be triggered when the worker returns.
            worker_handle.set_onmessage(Some(persistent_callback_handle.as_ref().unchecked_ref()));
        } else {
            console::log_1(&"uncompress locally".into());
            // let keys = UncompressedKeys::from(keys);
            let keys = UncompressedKeys(
                keys.0
                    .into_par_iter()
                    .map(|key| UncompressedPublicKey(key.uncompress().unwrap()))
                    .collect(),
            );
            console::log_1(&format!("uncompressed {} keys", keys.0.len()).into());
        }
    });

    // Attach the closure as `oninput` callback to the button.
    document
        .get_element_by_id("run")
        .expect("#run should exist")
        .dyn_ref::<HtmlElement>()
        .expect("#run should be a HtmlInputElement")
        .set_onclick(Some(callback.as_ref().unchecked_ref()));

    // Leaks memory.
    callback.forget();
}

/// Create a closure to act on the message returned by the worker
fn get_on_msg_callback() -> Closure<dyn FnMut(MessageEvent)> {
    Closure::new(move |event: MessageEvent| {
        console::log_1(&"Received response".into());
        let data = event.data();
        let keys: UncompressedKeys = serde_wasm_bindgen::from_value(data).unwrap();
        console::log_1(&format!("Deserialized {} keys", keys.0.len()).into());
    })
}
