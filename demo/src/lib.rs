use send_wrapper::SendWrapper;
use tokio::sync::OnceCell;
use wasm_bindgen::{prelude::wasm_bindgen, JsCast, UnwrapThrowExt};
use web_sys::{console, HtmlElement, HtmlInputElement};
use webworker::{webworker, WebWorker};
use webworker_proc_macro::webworker_fn;

#[webworker_fn]
pub fn sort(mut compressed: Box<[u8]>) -> Box<[u8]> {
    compressed.sort();
    compressed
}

async fn worker() -> &'static WebWorker {
    static WORKER: OnceCell<SendWrapper<WebWorker>> = OnceCell::const_new();
    WORKER
        .get_or_init(move || async {
            SendWrapper::new(
                WebWorker::new("/pkg/webworker_demo.js", None)
                    .await
                    .unwrap(),
            )
        })
        .await
}

/// Run entry point for the main thread.
#[wasm_bindgen]
pub async fn run() {
    let document = web_sys::window().unwrap().document().unwrap();

    let input_field = document
        .get_element_by_id("num_values")
        .expect("#num_keys should exist");
    let input_field = input_field
        .dyn_ref::<HtmlInputElement>()
        .expect("#num_keys should be a HtmlInputElement");

    // If the value in the field can be parsed to a `usize`, prepare the compressed keys.
    let num_values = input_field.value().parse::<usize>().unwrap_or(1);
    let mut values: Vec<u8> = vec![];
    for _ in 0..num_values {
        values.push(rand::random());
    }

    let worker = worker().await;
    // Access worker behind shared handle, following the interior
    // mutability pattern.
    console::log_1(&"postMessage to worker".into());
    let res = worker
        .run(webworker!(sort), &values)
        .await
        .expect_throw("Invalid result");

    let result_field = document
        .get_element_by_id("result")
        .expect("#result should exist");
    let result_field = result_field
        .dyn_ref::<HtmlElement>()
        .expect("#result should be a HtmlInputElement");
    result_field.set_inner_text(&format!("{:?} -> {:?}", values, res));
}
