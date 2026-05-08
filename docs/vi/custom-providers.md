# Cấu hình Provider Tùy chỉnh

Sau ZS-ML-015, provider chat tùy chỉnh phải đi qua manifest ai-protocol và được gọi bằng ID `provider/model`.

## Các loại Provider

### Endpoint chat có manifest

Thêm endpoint vào checkout ai-protocol, rồi cấu hình ID logic:

```toml
default_provider = "local-gateway/my-model"
default_model = "local-gateway/my-model"
```

Các cú pháp chat cũ `custom:https://...` và `anthropic-custom:https://...` hiện trả về lỗi hướng dẫn migration.

## Phương thức cấu hình

### File Config

Chỉnh sửa `~/.zeroclaw/config.toml`:

```toml
default_provider = "local-gateway/my-model"
default_model = "local-gateway/my-model"
```

### Biến môi trường

Dùng biến môi trường credential được khai báo trong manifest:

```bash
export API_KEY="your-api-key"
# hoặc: export ZEROCLAW_API_KEY="your-api-key"
zeroclaw agent
```

## Kiểm tra cấu hình

Xác minh endpoint tùy chỉnh của bạn:

```bash
# Chế độ tương tác
zeroclaw agent

# Kiểm tra tin nhắn đơn
zeroclaw agent -m "test message"
```

## Xử lý sự cố

### Lỗi xác thực

- Kiểm tra lại API key
- Kiểm tra định dạng URL endpoint (phải bao gồm `http://` hoặc `https://`)
- Đảm bảo endpoint có thể truy cập từ mạng của bạn

### Không tìm thấy Model

- Xác nhận tên model khớp với các model mà provider cung cấp
- Kiểm tra tài liệu của provider để biết định danh model chính xác
- Đảm bảo endpoint và dòng model khớp nhau. Một số gateway tùy chỉnh chỉ cung cấp một tập con model.
- Xác minh các model có sẵn từ cùng endpoint và key đã cấu hình:

```bash
curl -sS https://your-api.com/models \
  -H "Authorization: Bearer $API_KEY"
```

- Nếu gateway không triển khai `/models`, gửi một request chat tối giản và kiểm tra thông báo lỗi model mà provider trả về.

### Sự cố kết nối

- Kiểm tra khả năng truy cập endpoint: `curl -I https://your-api.com`
- Xác minh cài đặt firewall/proxy
- Kiểm tra trang trạng thái của provider

## Ví dụ

### LLM Server cục bộ

```toml
default_provider = "local-gateway/local-model"
default_model = "local-gateway/local-model"
```

### Proxy của doanh nghiệp

```toml
default_provider = "corp-proxy/claude-sonnet"
default_model = "corp-proxy/claude-sonnet"
```

### Cloud Provider Gateway

```toml
default_provider = "cloud-gateway/gpt-4"
default_model = "cloud-gateway/gpt-4"
```
