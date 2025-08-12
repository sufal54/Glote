#[macro_export]
macro_rules! mid {
    (
        $req:ident,
        $res:ident,
        $next:ident,
        $($body:tt)*
    ) => {
        |$req: Req, $res: Res, $next: Next| {
            ::std::boxed::Box::pin(async move {
                $($body)*
            }) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()> + Send>>
        }
    };
}

#[macro_export]
macro_rules! han {
    (
        $req:ident,
        $res:ident,
        $($body:tt)*
    ) => {
        |$req: Req, $res: Res| {
            ::std::boxed::Box::pin(async move {
                $($body)*
            }) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()> + Send>>
        }
    };
}
