// Shamelessly taken from
// https://stackoverflow.com/questions/36258417/using-a-macro-to-initialize-a-big-array-of-non-copy-elements
// Allows us to fill an array with a predefined value.
#[macro_export]
macro_rules! filled_array {
    (@accum (0, $($_es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@as_expr [$($body)*])};
    (@accum (1, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (0, $($es),*) -> ($($body)* $($es,)*))};
    (@accum (2, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (0, $($es),*) -> ($($body)* $($es,)* $($es,)*))};
    (@accum (3, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (2, $($es),*) -> ($($body)* $($es,)*))};
    (@accum (4, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (2, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (5, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (4, $($es),*) -> ($($body)* $($es,)*))};
    (@accum (6, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (4, $($es),*) -> ($($body)* $($es,)* $($es,)*))};
    (@accum (7, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (4, $($es),*) -> ($($body)* $($es,)* $($es,)* $($es,)*))};
    (@accum (8, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (4, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (16, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (8, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (32, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (16, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (64, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (32, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (128, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (64, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (256, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (128, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (512, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (256, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (1024, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (512, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (2048, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (1024, $($es,)* $($es),*) -> ($($body)*))};
    (@accum (4096, $($es:expr),*) -> ($($body:tt)*))
        => {filled_array!(@accum (2048, $($es,)* $($es),*) -> ($($body)*))};

    (@as_expr $e:expr) => {$e};

    [$e:expr; $n:tt] => { filled_array!(@accum ($n, $e) -> ()) };
}
