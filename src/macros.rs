#[macro_export]
macro_rules! debug {
    ($msg:expr) => {
        $crate::logger::get_logger().debug($msg);
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::logger::get_logger().debug(&format!($fmt, $($arg)*));
    };
}

#[macro_export]
macro_rules! info {
    ($msg:expr) => {
        $crate::logger::get_logger().info($msg);
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::logger::get_logger().info(&format!($fmt, $($arg)*));
    };
}

#[macro_export]
macro_rules! error {
    ($msg:expr) => {
        $crate::logger::get_logger().error($msg);
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::logger::get_logger().error(&format!($fmt, $($arg)*));
    };
}
