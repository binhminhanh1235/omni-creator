# Worker pool cho OmniCreator

Phase 19 P3 dùng một coordinator agent để giữ toàn bộ quyền inspect/prepare/commit với OmniCreator, còn các worker chỉ thực thi công việc bên ngoài theo descriptor đã được prepare.

Nguyên tắc cốt lõi: **chạy song song ở ngoài, commit canonical tuần tự ở trong OmniCreator**.

Coordinator gọi `agent_work_graph`, chọn `work_id` đang sẵn sàng, gọi `agent_work_prepare`, fan-out worker có giới hạn, nhận candidate result rồi gọi `agent_work_commit`. Sau mỗi batch commit phải gọi lại `agent_work_graph` để lấy state mới nhất.

Worker không được gọi `agent_work_commit`, không sửa Data Root/SQLite/ArtifactStore, không tự tạo ID, không tự đánh dấu stage thành công. Worker chỉ trả candidate result cùng provenance trung thực.

Luồng đầy đủ:

1. commit Script/Content;
2. commit ScenePlan/storyboard;
3. inspect work graph;
4. prepare các visual scene và voice segment đang READY;
5. chạy worker song song, mặc định bắt đầu từ tối đa 4 worker;
6. gom candidate result;
7. coordinator commit theo batch nhỏ;
8. inspect lại graph;
9. xử lý `stale_input`, `writer_conflict`, missing/failed work bằng recovery đúng loại;
10. chỉ tạo ProductionPack/Resolve sau khi fan-in canonical đã hoàn tất.

Sau reconnect, không dùng queue cache làm authority. Hãy dựng lại queue từ graph hiện tại. Nếu `stale_input`, bỏ kết quả cũ và prepare lại work hiện tại. Nếu `writer_conflict`, backoff rồi retry qua coordinator, không mở writer thứ hai.

Machine-readable contract: `agent-harness/worker-pool/contract.json`.
Recipe riêng cho từng harness nằm tại `agent-harness/codex/WORKER-POOL.md`, `agent-harness/claude/WORKER-POOL.md`, và `agent-harness/antigravity/WORKER-POOL.md`.
