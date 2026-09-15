# Gói tích hợp AI agent cho OmniCreator

Tài liệu chính và các lệnh cấu hình chi tiết nằm trong `agent-harness/README.md`. Bản tóm tắt này giúp kiểm tra nhanh cách dùng an toàn.

- Codex và Google Antigravity dùng skill tại `.agents/skills/omnicreator/SKILL.md`.
- Claude Code dùng skill tại `.claude/skills/omnicreator/SKILL.md`.
- Cả ba đều gọi cùng tiến trình local stdio: `omnicreator --data-root <path> mcp serve`.
- Nếu dùng LLM tự động, thêm `--llm-provider-config <path>` trước `mcp serve`; API key chỉ nằm trong biến môi trường của máy.
- Luôn inspect trạng thái trước khi mutate và inspect lại ngay sau mỗi mutation.
- Không sửa trực tiếp SQLite, ArtifactStore hoặc file nội bộ Data Root.
- Khi AUTO OFF hoặc provider/plugin/compute không sẵn sàng, dùng manual/external takeover tương ứng thay vì giả lập success.
- Chỉ báo hoàn tất khi OmniCreator xác nhận trạng thái canonical cuối cùng, ví dụ ProductionPack/export hợp lệ.

## Worker pool song song — Phase 19

Nguyên tắc là **parallel execution, serialized canonical commits**.

Một coordinator agent giữ quyền điều khiển OmniCreator: gọi `agent_work_graph`, `agent_work_prepare`, gom kết quả từ worker và gọi `agent_work_commit`. Worker chỉ thực hiện một descriptor đã prepare và trả candidate result về coordinator; worker không được tự commit, sửa Data Root, SQLite, ArtifactStore hoặc tự đánh dấu stage thành công.

Luồng khuyến nghị:

1. commit Script/Content vào state canonical;
2. commit ScenePlan/storyboard;
3. coordinator inspect work graph;
4. prepare các `work_id` đang READY;
5. fan-out visual theo scene và voice theo segment với số worker có giới hạn, mặc định bắt đầu từ 4;
6. worker trả candidate result + provenance;
7. coordinator commit theo batch nhỏ, tuần tự qua canonical writer;
8. inspect lại graph sau mỗi batch;
9. nếu `stale_input` thì bỏ candidate cũ và prepare lại; nếu `writer_conflict` thì backoff, không mở writer thứ hai;
10. QA/recovery rồi mới fan-in sang ProductionPack/Resolve.

Concurrency chỉ là cấu hình local của harness, không phải workflow truth được persist vào OmniCreator. Sau reconnect phải dựng lại queue từ graph hiện tại, không replay queue cache như authority.

Contract dùng chung: `agent-harness/worker-pool/contract.json`.
Hướng dẫn chi tiết: `agent-harness/worker-pool/README.md`.
Recipe riêng: `agent-harness/codex/WORKER-POOL.md`, `agent-harness/claude/WORKER-POOL.md`, `agent-harness/antigravity/WORKER-POOL.md`.

## Các file mẫu cấu hình

- Codex: `agent-harness/codex/config.toml.example`
- Claude Code: `agent-harness/claude/.mcp.json.example`
- Antigravity: `agent-harness/antigravity/mcp_config.json.example`
- OpenRouter: `agent-harness/providers/openrouter.json.example`
- LLMGateway: `agent-harness/providers/llmgateway.json.example`

Kịch bản recovery tham chiếu: `agent-harness/fixtures/blocked-workflow-recovery.json`.
