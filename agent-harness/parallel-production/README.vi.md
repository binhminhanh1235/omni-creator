# Recipe sản xuất song song visual + voice

Tracking: Phase 19 P4 #118.

Recipe này biến worker-pool của Phase 19 thành luồng sản xuất thực tế nhưng không tạo thêm workflow engine. OmniCreator vẫn là nguồn sự thật cho readiness, artifact canonical được chọn, stale detection, recovery và fan-in cuối cùng.

## Dependency gate

Voice segment có thể chạy bên ngoài ngay sau khi Content đã được xác minh. Visual scene chỉ có thể chạy sau khi ScenePlan đã được xác minh. Vì vậy coordinator có thể cho voice chạy song song với quá trình tạo ScenePlan, rồi bổ sung visual workers ngay khi ScenePlan được commit.

```text
Content --------------------------+--> voice segment workers
   |                              |
   `--> ScenePlan --> visual scene workers
                    \             /
                     canonical fan-in
                           |
                     ProductionPack
```

Luôn dựng lại hàng đợi worker từ `agent_work_graph`. Không suy diễn readiness từ transcript của harness.

## Visual dispatch: giữ đúng ba lane khác nhau

### 1. Stock lane: Pexels

Pexels là stock provider, không phải provider `visual.generate`. Manifest hiện tại khai báo `stock_video`, `stock_image`, `preview_first_search` và `selected_asset_download`.

Khi Studio Pack canonical và runtime capability chọn stock, dùng thứ tự:

1. Search bằng `visual.resolve` từ search intent của scene.
2. Chỉ giữ metadata/preview ở bước discovery. Xem candidate metadata, preview, creator/source metadata và `selection_ref`.
3. Chọn đúng một candidate. Không tải full-resolution cho mọi search result.
4. Chỉ fetch candidate đã chọn bằng `visual.fetch_selected`.
5. Commit image/video đã chọn qua một canonical OmniCreator visual path.

Nếu machine-local OmniCreator plugin runtime thực hiện stock execution, selection/fetch phải nằm trong runtime và visual orchestration canonical đó.

Nếu harness thực hiện Pexels như external fallback, ingest image/video đã chọn qua `visual_control` action `provide_manual`, với `ManualResultProvenanceV1` producer `EXTERNAL_RESULT_IMPORT` và source label trung thực như `pexels`. Path này hỗ trợ cả image và video.

Không dùng `agent_work_commit` với `ExternalGeneratedVisualRequestV1` để submit Pexels video. External visual descriptor của Phase 19 hiện là generated-still contract và chỉ nhận PNG/JPEG/WebP.

Nếu Pexels bị rate limit, giữ failure ở trạng thái retryable, tôn trọng `retry_after_seconds`, rồi retry hoặc chọn fallback được cho phép. Rate limit không bao giờ được chuyển thành success.

### 2. Generated-still lane

Khi visual work item chuẩn bị sẵn descriptor `visual.generate`, worker có thể render still image theo descriptor đó. Kết quả phải là PNG, JPEG hoặc WebP.

Coordinator commit qua `agent_work_commit` hoặc external handoff tương đương của `visual_control`. `request_sha256` bảo vệ khỏi trường hợp ScenePlan thay đổi trong khi worker đang render.

Generated-still không đại diện cho stock capability và không được dùng để đưa video vào still-image contract.

### 3. Manual visual lane

Khi stock/generated provider không khả dụng, hoặc người dùng có asset phù hợp hơn, ingest image/video bằng canonical manual visual operation. Provenance phải trung thực và replacement phải explicit nếu scene đã có verified result.

Manual takeover là production path được hỗ trợ, không phải workaround ẩn.

## Voice dispatch: OmniVoiceStudio

Cho mỗi READY voice work item:

1. Coordinator gọi `agent_work_prepare` cho canonical voice `work_id`.
2. Gửi descriptor `tts.generate` trả về cho OmniVoiceStudio worker. Descriptor chứa `project_id`, `segment_id`, narration, voice direction, result contract và `request_sha256` canonical.
3. OmniVoiceStudio render WAV hoặc MP3 cùng timing tương ứng. Timing là bắt buộc và phải tương thích SRT hoặc OmniCreator VoiceTiming JSON của chính segment đó.
4. Worker chỉ trả candidate path/result metadata về coordinator. Worker không được mutate OmniCreator.
5. Coordinator submit qua `agent_work_commit` với source label `omnivoice-studio`.
6. Chạy lại `agent_work_graph` ngay sau commit.

Nếu Content thay đổi trong lúc render, OmniCreator phải reject request hash cũ là stale. Output cũ không được coi là current result; prepare lại work identity mới.

Nếu OmniVoiceStudio hoặc compute route không khả dụng, dùng `voice_control` action `provide_manual` với audio + timing hợp lệ. Không đánh dấu voice unit complete chỉ vì external renderer đã tạo file.

## Chính sách song song

Concurrency của provider là cấu hình vận hành, không phải portable Project truth.

- Giới hạn tổng worker theo P3 worker-pool policy.
- Pexels search/fetch concurrency cần bảo thủ và phải tuân thủ retry guidance của provider.
- OmniVoiceStudio concurrency phải bám capacity thực của local/remote runtime, kể cả GPU/ComputeProvider.
- Credential, provider path, device ID và rate-limit state phải ở machine-local/harness-local.
- Canonical commit vẫn tuần tự qua coordinator và Data Root writer lease.

Cách scheduling hiệu quả là giữ voice workers chạy ngay khi Content sẵn sàng, rồi đưa visual workers vào theo từng wave sau khi ScenePlan sẵn sàng. Không cần chờ toàn bộ voice xong mới chạy visual và ngược lại.

## Ví dụ mixed fallback

**Pexels unavailable:** giữ provider failure trung thực. Nếu canonical Studio Pack/review cho phép generated still, dispatch generated visual; nếu không thì dùng manual visual.

**Pexels search có kết quả nhưng candidate không đạt:** không tải một full-media ngẫu nhiên chỉ để có success. Chọn provider/route khác hoặc manual takeover.

**Generated provider unavailable:** cung cấp supported external/manual still image, không biến failed provider Attempt thành success.

**OmniVoiceStudio unavailable:** cung cấp manual audio + timing bundle với provenance đúng.

**Một worker fail, worker khác success:** chỉ commit candidate đã hoàn tất và được chấp nhận, re-inspect rồi redispatch phần còn thiếu vẫn còn current. P5 sẽ harden sâu hơn crash/contention/fan-in; P4 không tạo thêm worker database.

## Fan-in gate

Trước ProductionPack:

1. re-inspect `agent_work_graph`;
2. dùng `visual_control status` và `voice_control status` khi cần chẩn đoán unit còn thiếu;
3. kiểm tra Review Center/recovery cho failed/stale unit;
4. yêu cầu canonical visual + voice aggregate đã verified;
5. chỉ sau đó mới assemble/rebuild/export ProductionPack.

Worker completion, Pexels download hay OmniVoiceStudio audio file chỉ là bằng chứng external execution. Canonical acceptance của OmniCreator mới là production truth.

Machine-readable rules nằm ở `agent-harness/parallel-production/contract.json`.
