# Runtime endpoint của OmniVoiceStudio

Tracking: Phase 19 #113, P5 #119, runtime endpoint slice #126.

## Vì sao cần cấu hình runtime

Khi OmniVoiceStudio được public qua Cloudflare quick tunnel, hostname `*.trycloudflare.com` có thể đổi sau mỗi lần Studio/tunnel khởi động lại. Vì vậy domain này không được hard-code vào OmniCreator và cũng không được lưu như project truth có tính portable.

OmniCreator coi địa chỉ OmniVoiceStudio là **cấu hình runtime riêng của máy hiện tại**.

## Đổi domain khi OmniCreator đang chạy

Trong Desktop, bấm nút **OmniVoiceStudio** trên thanh trên cùng. Có thể paste trực tiếp bất kỳ URL nào mà OmniVoiceStudio in ra:

```text
https://example.trycloudflare.com/ui
https://example.trycloudflare.com/api/v1
https://example.trycloudflare.com/mcp
```

Hoặc chỉ paste root domain:

```text
https://example.trycloudflare.com
```

Bấm **Save & connect**. Không cần restart OmniCreator. OmniCreator tự normalize về root domain rồi suy ra các endpoint hiện hành:

```text
Studio UI:  <base>/ui
REST API:   <base>/api/v1
MCP:        <base>/mcp
Health:     <base>/health
Capabilities: <base>/api/v1/capabilities
Audio generation: <base>/api/v1/audio/generate
```

Lần refresh/runtime operation tiếp theo đọc cấu hình máy hiện tại, nên khi thay domain tunnel mới thì thay đổi có hiệu lực ngay.

## Authentication

REST/MCP public của OmniVoiceStudio có thể yêu cầu bearer token. OmniCreator mặc định dùng đúng tên biến môi trường mà OmniVoiceStudio dùng:

```text
OMNIVOICE_API_TOKEN
```

OmniCreator chỉ lưu **tên biến môi trường**, không lưu giá trị token. Giá trị token chỉ được đọc từ environment của process lúc probe runtime và không được ghi vào config của OmniCreator, Data Root, project artifact, external descriptor, log hay handoff state.

Nếu Studio chủ động cho phép machine API không cần auth, có thể để trống ô bearer-token environment variable.

## Kiểm tra runtime thật

`Save & connect` và `Refresh` không chỉ lưu hostname mà còn kiểm tra target thật:

1. `GET <base>/health`
2. `GET <base>/api/v1/capabilities`
3. xác minh runtime quảng bá standalone audio generation
4. hiển thị trạng thái MCP và lỗi bearer auth một cách trung thực

Panel trả về `READY`, `NEEDS API TOKEN`, `OFFLINE` hoặc `DEGRADED`, không biến tunnel/provider failure thành success giả.

## Ranh giới portability

Cấu hình máy được lưu trong Tauri application config directory với tên `omnivoice-studio.json`, nằm ngoài portable OmniCreator Data Root.

Ranh giới Phase 19 vẫn được giữ nguyên:

- Project/Data Root sở hữu workflow truth và artifact truth chuẩn.
- Tunnel URL, credential và provider capacity của OmniVoiceStudio là runtime fact riêng của máy/harness.
- Đổi tunnel domain không rewrite lịch sử project.
- External voice result chỉ trở thành canonical sau khi đi qua result-ingress/validation hiện có của OmniCreator.

## Các dạng URL được chấp nhận

Normalizer chấp nhận root URL và các surface `/ui`, `/api/v1`, `/mcp`, `/health`, `/api/v1/capabilities`. Nó từ chối:

- URL không phải HTTP(S);
- URL nhúng credential dạng `user:password@host`;
- query string hoặc fragment;
- path lạ ngoài contract Studio.

Nhờ vậy runtime locator luôn xác định được một cách deterministic và không vô tình nhét secret vào endpoint string.
