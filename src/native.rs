day_bridge::bridge! {
    #[day_bridge::declare]
    extern "day" {
        fn initialize_native(identifiers: &str) -> Result<(), day_bridge::Error>;
        fn events_native(events: day_bridge::Emit<String>) -> Result<(), day_bridge::Error>;
        fn schedule_native(request: &str, done: day_bridge::Done<String>) -> Result<(), day_bridge::Error>;
        fn cancel_native(identifier: &str, done: day_bridge::Done<String>) -> Result<(), day_bridge::Error>;
        fn finish_native(run: i64, result: &str) -> Result<(), day_bridge::Error>;
    }
    #[day_bridge::impl(swift, platforms = [ios], src = "platform/Background.swift")]
    #[day_bridge::impl(java, platforms = [android], src = "platform/Background.java")]
    #[day_bridge::impl(rust, platforms = [other])]
    fn initialize_native(_identifiers: &str) -> Result<(), day_bridge::Error> { Err(day_bridge::Error::Unsupported) }
    #[day_bridge::impl(rust, platforms = [other])]
    fn events_native(_events: day_bridge::Emit<String>) -> Result<(), day_bridge::Error> { Err(day_bridge::Error::Unsupported) }
    #[day_bridge::impl(rust, platforms = [other])]
    fn schedule_native(_request: &str, _done: day_bridge::Done<String>) -> Result<(), day_bridge::Error> { Err(day_bridge::Error::Unsupported) }
    #[day_bridge::impl(rust, platforms = [other])]
    fn cancel_native(_identifier: &str, _done: day_bridge::Done<String>) -> Result<(), day_bridge::Error> { Err(day_bridge::Error::Unsupported) }
    #[day_bridge::impl(rust, platforms = [other])]
    fn finish_native(_run: i64, _result: &str) -> Result<(), day_bridge::Error> { Err(day_bridge::Error::Unsupported) }
}
pub(crate) fn initialize(ids: &str) -> Result<(), day_bridge::Error> {
    initialize_native(ids)
}
pub(crate) fn finish(run: i64, result: &str) -> Result<(), day_bridge::Error> {
    finish_native(run, result)
}
