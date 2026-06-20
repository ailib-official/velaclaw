//! BYOK usage telemetry — async UsageRecord posts to Prism (VL-EVO-003).
//! BYOK 用量遥测：成功调用后异步上报，默认不含 key / prompt 正文。

mod record;
mod reporter;

pub use record::{parse_token_counts, ByokUsageRecord, FORBIDDEN_PAYLOAD_KEYS};
pub use reporter::ByokTelemetryReporter;
