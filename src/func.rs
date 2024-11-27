pub struct WebWorkerFn {
    pub(crate) name: &'static str,
}

impl WebWorkerFn {
    pub fn new_unchecked(func_name: &'static str, _f: fn(Box<[u8]>) -> Box<[u8]>) -> Self {
        Self { name: func_name }
    }

    pub fn from_name_unchecked(func_name: &'static str) -> Self {
        Self { name: func_name }
    }
}

#[macro_export]
macro_rules! webworker {
    ($name:ident) => {{
        let _ = $name::__WEBWORKER;
        $crate::func::WebWorkerFn::new_unchecked(stringify!($name), $name)
    }};
}
