# Tài liệu Bắt đầu

Dành cho cài đặt lần đầu và làm quen nhanh.

## Lộ trình bắt đầu

1. Tổng quan và khởi động nhanh: [../../../README.vi.md](../../../README.vi.md)
2. Cài đặt một lệnh và chế độ bootstrap kép: [../one-click-bootstrap.md](../one-click-bootstrap.md)
3. Tìm lệnh theo tác vụ: [../commands-reference.md](../commands-reference.md)

## Chọn hướng đi

| Tình huống | Lệnh |
|----------|---------|
| Có API key, muốn cài nhanh nhất | `velaclaw onboard --api-key sk-... --provider openai/gpt-5.2` |
| Muốn được hướng dẫn từng bước | `velaclaw onboard --interactive` |
| Đã có config, chỉ cần sửa kênh | `velaclaw onboard --channels-only` |
| Dùng xác thực subscription | Xem [Subscription Auth](../../../README.md#subscription-auth-openai-codex--claude-code) |

## Thiết lập và kiểm tra

- Thiết lập nhanh: `velaclaw onboard --api-key "sk-..." --provider openai/gpt-5.2`
- Thiết lập tương tác: `velaclaw onboard --interactive`
- Kiểm tra môi trường: `velaclaw status` + `velaclaw doctor`

## Tiếp theo

- Vận hành runtime: [../operations/README.md](../operations/README.md)
- Tra cứu tham khảo: [../reference/README.md](../reference/README.md)
