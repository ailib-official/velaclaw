# Tham khảo lệnh VelaClaw

Tài liệu này dựa trên giao diện CLI hiện tại (`velaclaw --help`).

Xác minh lần cuối: **2026-02-20**.

## Lệnh cấp cao nhất

| Lệnh | Mục đích |
|---|---|
| `onboard` | Khởi tạo workspace/config nhanh hoặc tương tác |
| `agent` | Chạy chat tương tác hoặc chế độ gửi tin nhắn đơn |
| `gateway` | Khởi động gateway webhook và HTTP WhatsApp |
| `daemon` | Khởi động runtime có giám sát (gateway + channels + heartbeat/scheduler tùy chọn) |
| `service` | Quản lý vòng đời dịch vụ cấp hệ điều hành |
| `doctor` | Chạy chẩn đoán và kiểm tra trạng thái |
| `status` | Hiển thị cấu hình và tóm tắt hệ thống |
| `cron` | Quản lý tác vụ định kỳ |
| `models` | Làm mới danh mục model của provider |
| `providers` | Liệt kê ID provider, bí danh và provider đang dùng |
| `channel` | Quản lý kênh và kiểm tra sức khỏe kênh |
| `integrations` | Kiểm tra chi tiết tích hợp |
| `skills` | Liệt kê/cài đặt/gỡ bỏ skills |
| `migrate` | Nhập dữ liệu từ runtime khác (hiện hỗ trợ OpenClaw) |
| `config` | Xuất schema cấu hình dạng máy đọc được |
| `completions` | Tạo script tự hoàn thành cho shell ra stdout |
| `hardware` | Phát hiện và kiểm tra phần cứng USB |
| `peripheral` | Cấu hình và nạp firmware thiết bị ngoại vi |

## Nhóm lệnh

### `onboard`

- `velaclaw onboard`
- `velaclaw onboard --interactive`
- `velaclaw onboard --channels-only`
- `velaclaw onboard --api-key <KEY> --provider <ID> --memory <sqlite|lucid|markdown|none>`
- `velaclaw onboard --api-key <KEY> --provider <ID> --model <MODEL_ID> --memory <sqlite|lucid|markdown|none>`

### `agent`

- `velaclaw agent`
- `velaclaw agent -m "Hello"`
- `velaclaw agent --provider <ID> --model <MODEL> --temperature <0.0-2.0>`
- `velaclaw agent --peripheral <board:path>`

### `gateway` / `daemon`

- `velaclaw gateway [--host <HOST>] [--port <PORT>]`
- `velaclaw daemon [--host <HOST>] [--port <PORT>]`

### `service`

- `velaclaw service install`
- `velaclaw service start`
- `velaclaw service stop`
- `velaclaw service restart`
- `velaclaw service status`
- `velaclaw service uninstall`

### `cron`

- `velaclaw cron list`
- `velaclaw cron add <expr> [--tz <IANA_TZ>] <command>`
- `velaclaw cron add-at <rfc3339_timestamp> <command>`
- `velaclaw cron add-every <every_ms> <command>`
- `velaclaw cron once <delay> <command>`
- `velaclaw cron remove <id>`
- `velaclaw cron pause <id>`
- `velaclaw cron resume <id>`

### `models`

- `velaclaw models refresh`
- `velaclaw models refresh --provider <ID>`
- `velaclaw models refresh --force`

`models refresh` hiện hỗ trợ làm mới danh mục trực tiếp cho các provider: `openrouter`, `openai`, `anthropic`, `groq`, `mistral`, `deepseek`, `xai`, `together-ai`, `gemini`, `ollama`, `astrai`, `venice`, `fireworks`, `cohere`, `moonshot`, `glm`, `zai`, `qwen` và `nvidia`.

### `channel`

- `velaclaw channel list`
- `velaclaw channel start`
- `velaclaw channel doctor`
- `velaclaw channel bind-telegram <IDENTITY>`
- `velaclaw channel add <type> <json>`
- `velaclaw channel remove <name>`

Lệnh trong chat khi runtime đang chạy (Telegram/Discord):

- `/models`
- `/models <provider>`
- `/model`
- `/model <model-id>`

Channel runtime cũng theo dõi `config.toml` và tự động áp dụng thay đổi cho:
- `default_provider`
- `default_model`
- `default_temperature`
- `api_key` / `api_url` (cho provider mặc định)
- `reliability.*` cài đặt retry của provider

`add/remove` hiện chuyển hướng về thiết lập có hướng dẫn / cấu hình thủ công (chưa hỗ trợ đầy đủ mutator khai báo).

### `integrations`

- `velaclaw integrations info <name>`

### `skills`

- `velaclaw skills list`
- `velaclaw skills install <source>`
- `velaclaw skills remove <name>`

`<source>` chấp nhận git remote (`https://...`, `http://...`, `ssh://...` và `git@host:owner/repo.git`) hoặc đường dẫn cục bộ.

Skill manifest (`SKILL.toml`) hỗ trợ `prompts` và `[[tools]]`; cả hai được đưa vào system prompt của agent khi chạy, giúp model có thể tuân theo hướng dẫn skill mà không cần đọc thủ công.

### `migrate`

- `velaclaw migrate openclaw [--source <path>] [--dry-run]`

### `config`

- `velaclaw config schema`

`config schema` xuất JSON Schema (draft 2020-12) cho toàn bộ hợp đồng `config.toml` ra stdout.

### `completions`

- `velaclaw completions bash`
- `velaclaw completions fish`
- `velaclaw completions zsh`
- `velaclaw completions powershell`
- `velaclaw completions elvish`

`completions` chỉ xuất ra stdout để script có thể được source trực tiếp mà không bị lẫn log/cảnh báo.

### `hardware`

- `velaclaw hardware discover`
- `velaclaw hardware introspect <path>`
- `velaclaw hardware info [--chip <chip_name>]`

### `peripheral`

- `velaclaw peripheral list`
- `velaclaw peripheral add <board> <path>`
- `velaclaw peripheral flash [--port <serial_port>]`
- `velaclaw peripheral setup-uno-q [--host <ip_or_host>]`
- `velaclaw peripheral flash-nucleo`

## Mẹo kiểm tra

Để xác minh nhanh tài liệu với binary hiện tại:

```bash
velaclaw --help
velaclaw <command> --help
```
