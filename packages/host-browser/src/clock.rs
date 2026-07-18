use keeless_core::Clock;

pub(crate) struct BrowserClock;

impl Clock for BrowserClock {
    fn now_millis(&self) -> i64 {
        js_sys::Date::now().min(i64::MAX as f64) as i64
    }

    fn monotonic_millis(&self) -> u64 {
        web_sys::window()
            .and_then(|window| window.performance())
            .map_or_else(js_sys::Date::now, |performance| performance.now())
            .max(0.0) as u64
    }
}
