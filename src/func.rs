use std::marker::PhantomData;

pub struct WebWorkerFn<T, R> {
    pub(crate) name: &'static str,
    _arg: PhantomData<T>,
    _res: PhantomData<R>,
}

impl<T, R> WebWorkerFn<T, R> {
    pub fn new_unchecked(func_name: &'static str, _f: fn(T) -> R) -> Self {
        Self::from_name_unchecked(func_name)
    }

    pub fn from_name_unchecked(func_name: &'static str) -> Self {
        Self {
            name: func_name,
            _arg: PhantomData,
            _res: PhantomData,
        }
    }
}

#[macro_export]
macro_rules! webworker {
    ($name:ident) => {{
        let _ = $name::__WEBWORKER;
        $crate::func::WebWorkerFn::new_unchecked(stringify!($name), $name)
    }};
}
