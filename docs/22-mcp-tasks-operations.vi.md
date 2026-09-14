# Vận hành MCP Tasks

Tài liệu này mô tả hành vi MCP Tasks bền vững của OmniCreator trong Phase 18 P5. Nội dung bổ sung cho `docs/19-mcp-control.md` và các gói agent trong `agent-harness/`.

## Transport và protocol

OmniCreator tiếp tục dùng `stdio` cục bộ làm control-plane transport của Phase 18 và target MCP `2026-07-28` thông qua `rmcp 3.3.0`.

Server quảng bá extension `io.modelcontextprotocol/tasks`. Một tool call chỉ được materialize thành Task khi client kết nối khai báo Tasks capability. Client không khai báo capability này vẫn nhận synchronous MCP tool response như trước.

Phase 18 P5 taskify `creator_start_or_resume`. Các MCP tool còn lại giữ nguyên contract hiện có.

## Task identity là canonical state

OmniCreator không lưu MCP Task trong database riêng. `taskId` chính là canonical OmniCreator `Job` ID. Audit trail sử dụng các record `Job`, `Attempt` và `Artifact` hiện có trong Data Root.

Thiết kế này bảo đảm:

- không có `mcp_tasks` shadow database hoặc process-local source of truth;
- restart server không làm mất task đã hoàn tất;
- di chuyển portable Data Root vẫn giữ trạng thái task và result artifact;
- client mới có thể dựng lại trạng thái task từ canonical state sau reconnect;
- công việc do agent khởi tạo dùng cùng audit/recovery surface với các execution path khác của OmniCreator.

Output terminal được persist thành verified artifact loại `control.task-result.v1` trong logical namespace của project. Artifact lưu MCP `CallToolResult` gốc. Vì vậy lỗi ở cấp tool như `provider_unavailable` vẫn là MCP Task `completed` nhưng result có `isError=true`. Điều này đúng với semantics MCP Tasks và không biến provider failure thành success.

## Lifecycle

Với client hỗ trợ Tasks:

1. Gọi `creator_start_or_resume`.
2. Server có thể trả `resultType: "task"` cùng durable `taskId`.
3. Poll `tasks/get` theo `pollIntervalMs` được gợi ý.
4. Terminal status là `completed`, `failed` hoặc `cancelled`.
5. Sau reconnect/restart, client hỗ trợ Tasks khác vẫn có thể gọi `tasks/get` với cùng task ID.

Mapping canonical:

| Canonical OmniCreator | MCP task |
| --- | --- |
| `READY`, `QUEUED`, `RUNNING` | `working` |
| `SUCCEEDED` | `completed` |
| `CANCELLED` | `cancelled` |
| interrupted/retryable/fatal/stale invalid terminal state | `failed` |

Hiện OmniCreator không tạo MCP task ở trạng thái `input_required`, vì vậy `tasks/update` bị từ chối. Khi cần can thiệp, hãy dùng typed manual/external takeover tool rồi gọi creator resume.

## Cancellation

`tasks/cancel` là cooperative cancellation và cập nhật canonical state. Control Job và active Attempt được chuyển cùng nhau sang `CANCELLED`; thao tác này không bao giờ đánh dấu provider hoặc workflow work thành success giả.

Cancellation không rollback những creator changes đã commit trước khi lệnh cancel đến. Verified artifact và workflow mutation đã hoàn tất vẫn được giữ nguyên. Task result không được promote đè lên trạng thái task đã cancel.

Read-only MCP session không được cancel task.

## Restart và interruption

Khi writable MCP server khởi động, OmniCreator chạy cơ chế canonical interrupted-job reconciliation đã có. Job/Attempt bị bỏ lại ở `RUNNING` sẽ chuyển sang trạng thái retryable/reconciliation hiện có thay vì treo `working` vô hạn hoặc bị giả thành success.

Task đã hoàn tất không bị ảnh hưởng bởi restart reconciliation vì Job/Attempt và verified result artifact đã terminal.

## Portable Data Root

Task state di chuyển cùng Data Root. Sau khi dừng MCP process, có thể move/sync Data Root theo quy tắc portability của OmniCreator. Khởi động server với đường dẫn Data Root mới và dùng lại cùng task ID qua `tasks/get`.

Không copy/move Data Root đang có writer active. Single-writer lease vẫn là authority chung cho Desktop, CLI và MCP.

## Security và privacy

P5 kế thừa response sanitizer và typed control errors hiện có. Operator và agent không được đưa credential vào project content, task ID, artifact metadata hoặc portable provider config.

Provider secret vẫn machine-local và chỉ được tham chiếu thông qua tên environment variable đã cấu hình. MCP response không được lộ raw bearer token, API-key value hoặc absolute Data Root path ngoài ý muốn.

## Compatibility

Client không khai báo Tasks extension tiếp tục dùng synchronous behavior của Phase 18 P2. Nhờ vậy các tích hợp stdio cũ vẫn hoạt động, trong khi client mới có thể opt-in durable Task handle.

Local stdio vẫn là transport được hỗ trợ trong Phase 18. Public/unauthenticated remote MCP control nằm ngoài scope.

## Kiểm tra vận hành

Trước khi coi một task là thành công, hãy đọc terminal `tasks/get` result rồi kiểm tra canonical project/workflow status tương ứng. MCP Task `completed` hoàn toàn có thể chứa tool result `isError=true`, ví dụ blocker `provider_unavailable` trung thực.

Khi recovery, dùng Review Center, manual/external takeover, retry/reconciliation và creator resume. Không chỉnh sửa trực tiếp SQLite database hoặc task-result artifact.
