# Hướng dẫn sử dụng OmniCreator (Tiếng Việt)

Tài liệu này hướng dẫn sử dụng phiên bản OmniCreator desktop hiện tại theo trạng thái đã hoàn tất và xác minh từ Phase 0 đến Phase 16. Nội dung ưu tiên góc nhìn người làm nội dung, đồng thời bao gồm các thao tác nâng cao để tiếp tục dự án khi LLM, provider, plugin, GPU, ComputeProvider hoặc dịch vụ bên ngoài không hoạt động.

> Nguyên tắc vận hành cốt lõi: tự động hóa chỉ là bộ tăng tốc, không được trở thành điểm nghẽn duy nhất. Mỗi stage đều có đường tiếp tục hoặc recovery thủ công, và mọi kết quả hợp lệ đều quay về cùng hệ thống canonical Project, WorkflowStep, Job, Attempt, Artifact và Production Pack.

## 1. OmniCreator dùng để làm gì?

OmniCreator là hệ thống local-first, plugin-driven để chuẩn bị một production video trước khi vào giai đoạn biên tập sáng tạo trong DaVinci Resolve.

OmniCreator có thể chuẩn bị và quản lý:

- topic hoặc script đầu vào
- content canonical và các segment
- scene plan / SceneIntent
- stock visual, generated visual, stick-figure, Asset Library hoặc media do bạn tự cung cấp
- narration audio và timing
- provenance, nguồn asset và thông tin license liên quan
- workflow state có thể resume/retry
- subtitle và dữ liệu timeline/interchange
- Production Pack sẵn sàng đưa sang DaVinci Resolve

OmniCreator **không thay thế DaVinci Resolve**. Việc dựng cuối, hiệu ứng, color grading, mixing và render cuối vẫn thuộc editor.

## 2. Trạng thái hiện tại và cách chạy ứng dụng

Repository hiện chưa có GitHub Release được publish. Desktop đang là ứng dụng Tauri 2 với frontend tĩnh nằm ở `apps/desktop/dist`.

Nếu chạy từ source, bạn cần:

- macOS là môi trường mục tiêu chính hiện tại
- Rust 1.80 trở lên
- Rust/Cargo toolchain hoạt động
- các dependency nền tảng cần thiết cho Tauri 2
- Python 3 nếu muốn trực tiếp chạy các visual plugin Python đang được check-in

Từ root của repository, có thể chạy desktop ở chế độ development bằng:

```bash
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Các lệnh kiểm tra Rust chính đang được CI sử dụng:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Các phần bên dưới giả định ứng dụng đã được mở.

## 3. Các khái niệm nên hiểu trước

### Data Root

Data Root là workspace bền vững của OmniCreator. Project, SQLite state, artifacts, exports, library media, Studio Packs và metadata portable đều nằm dưới thư mục này. Canonical state dùng logical reference thay vì phụ thuộc đường dẫn tuyệt đối của một máy cụ thể.

### Studio Pack

Studio Pack là cấu hình phong cách production ở tầng người dùng. Nó mô tả capability route, automation level, preset và quality threshold. Thông thường bạn chọn Studio Pack thay vì tự nối từng provider.

### Project Kanban

Project board được suy ra từ canonical workflow state. Các cột hiện tại:

- Ideas
- Preparing
- Needs Review
- GPU Ready
- GPU Running
- Ready to Edit
- Done

Vị trí của project trên board không phải một state UI riêng. Sau restart hoặc chuyển Data Root sang máy khác, board được dựng lại từ Project, WorkflowStep và Job hiện có.

### Review Center

Review Center là nơi chính để xử lý exception và recovery. Nó cho biết stage nào đang blocked, failed, unavailable, thiếu input hoặc có artifact invalid, sau đó chỉ hiển thị những action thực sự hợp lệ.

### Production Pack

Production Pack là gói dữ liệu và media đã được xác minh để đưa dự án vào DaVinci Resolve ở trạng thái gần với giai đoạn creative editing thay vì bắt đầu từ timeline trống.

## 4. Lần chạy đầu tiên và thiết lập Data Root

Ở lần chạy đầu, OmniCreator cho hai lựa chọn:

- **Create New Data Folder**
- **Use Existing Data Folder**

Chọn **Create New Data Folder** nếu tạo workspace mới.

Chọn **Use Existing Data Folder** nếu đã có Data Root OmniCreator và muốn mở lại trên cùng máy hoặc máy khác.

Một workspace hợp lệ có `.omnicreator/workspace.json` cùng state bền vững bên dưới Data Root. Nội dung thông thường gồm project state, media, artifacts, Studio Packs, Asset Library, exports và snapshot handoff sạch.

### Khi OmniCreator nghi ngờ còn writer khác

OmniCreator không tự động mở writer thứ hai trên cùng workspace đồng bộ. Nếu phát hiện khả năng conflict, dùng một trong các lựa chọn:

- **Open Read Only** để xem an toàn
- **Check Again** sau khi đóng OmniCreator ở máy còn lại và chờ sync xong
- **Choose Different Folder** nếu đã chọn nhầm workspace

Ở read-only mode, bạn vẫn xem được project, Review Center, Production Pack, recovery health và Details. Các action thay đổi dữ liệu sẽ bị khóa, và backend writable guard vẫn là lớp kiểm soát cuối cùng.

## 5. Workspace chính và Project Kanban

Mỗi project card hiển thị status ngắn gọn, Studio Pack, stage strip và action chính tiếp theo. Các stage semantic chính:

1. Content
2. Scenes
3. Visuals
4. Voice
5. Production Pack

Tùy trạng thái, nút chính có thể là:

- **Start / Resume**
- **Review**
- **Run GPU**
- **Sync / Resume**
- **Assemble**
- **Review / Export**

Project card còn có **Studio Pack**, **Production Status** hoặc **Export to Resolve**, cùng Rename/Delete nếu workspace đang writable.

## 6. Cấu hình integration và plugin

Bạn không cần có đầy đủ mọi integration mới hoàn thành được video. Chỉ cấu hình những capability tự động mà bạn muốn dùng. Capability bị thiếu phải trở thành setup/recovery action, không phải ngõ cụt của project.

### LLMGateway

LLMGateway được dùng cho các tác vụ tạo content và SceneIntent tự động. Dùng panel **LLMGateway** để kiểm tra/cấu hình kết nối local gateway.

Nếu LLMGateway không hoạt động, bạn vẫn có thể tiếp tục bằng cách tự cung cấp Script và Scene Plan. Provider/model/session không được lưu thành portable Project state.

### Plugin Manager

Mở **Plugin Manager** từ khu vực workspace settings. UI hiện có các nhóm:

- **Installed / Enabled**
- **Disabled**
- **Needs Attention**

Built-in plugin có thể bật/tắt nhưng việc update thuộc application distribution. User-installed plugin có thể có flow Inspect Update và Uninstall. Trạng thái cài plugin là machine-local, không phải portable project truth.

### Các visual provider đang được check-in

| Provider | Mục đích chính | Biến môi trường / cấu hình secret mặc định |
| --- | --- | --- |
| Pexels | Stock image/video, preview-first | `PEXELS_API_KEY` |
| Pixabay | Stock image/video, preview-first | `PIXABAY_API_KEY` |
| Unsplash | Stock photo có attribution và selected-use tracking | `UNSPLASH_ACCESS_KEY` |
| Storyblocks | Commercial stock image/video | `STORYBLOCKS_PUBLIC_KEY`, `STORYBLOCKS_PRIVATE_KEY`, `STORYBLOCKS_USER_ID`, `STORYBLOCKS_API_MODE` |
| Generated Image API | Generated still qua API | mặc định dùng `OPENAI_API_KEY`; endpoint/model cấu hình trong plugin settings |
| Generated Image Reference | Generated-still reference/offline | implementation reference không yêu cầu provider secret |
| Stick Figure Reference | Stick-figure visual offline | implementation reference không yêu cầu provider secret |

Secret phải nằm machine-local. Không ghi API key/token vào Project, Studio Pack, ScenePlan, external-handoff JSON hoặc canonical portable state bên trong Data Root.

Với Storyblocks, test credential có thể search/preview nhưng chỉ production API mode và license phù hợp mới được promote selected production asset.

### Voice và ComputeProvider

Voice tự động dùng voice runtime đã cấu hình và, khi cần, ComputeProvider/GPU. Nếu không có runtime/GPU, bạn có thể import audio và timing thủ công. GPU không phải điều kiện bắt buộc để mọi project hoàn thành.

## 7. Chọn Studio Pack và tạo production

Settings dành cho creator có ba tầng.

### Basic

Dùng Basic cho flow thông thường. Chọn creator input và Studio Pack khả dụng, sau đó tạo/start production.

Một Studio Pack có thể ở trạng thái:

- `AVAILABLE`
- `AVAILABLE_WITH_SETUP`
- `UNAVAILABLE`

Availability được tính từ plugin capability thật và readiness machine-local. Việc một Studio Pack có tên trong catalog không có nghĩa capability bị thiếu tự nhiên trở thành khả dụng.

### Customize

Dùng Customize để chỉnh các tham số có giá trị cao như:

- curated preset
- automation level
- quality threshold

Project customization vẫn là portable Studio Pack child kế thừa pack gốc, không tạo ra routing engine mới.

### Advanced

Advanced dùng để xem resolved plugin/capability route, thứ tự target, nguồn giá trị, diagnostics và runtime controls machine-local. Provider endpoint, credential value và absolute path không được lưu vào portable Studio Pack state.

### Automation level

- **Assisted**: không tự vượt qua creator review checkpoint.
- **Balanced**: tự xử lý phần deterministic/low-risk và dừng ở quyết định đáng review hoặc ambiguous.
- **Autopilot**: tự tiến qua công việc low-risk, nhưng vẫn dừng nếu có blocker hoặc exception ảnh hưởng lớn.

Cả ba mode đều tuân theo capability/setup error và canonical workflow state machine.

## 8. Flow tự động chuẩn

Một flow tự động bình thường:

```text
Topic hoặc Script
  -> Content
  -> Scene Plan / SceneIntent
  -> Chuẩn bị và chọn Visual
  -> Voice / Timing
  -> Production Pack
  -> Export sang Resolve
```

### Content

Ở topic mode, OmniCreator có thể gọi LLMGateway để tạo canonical creator content. Ở script mode, nội dung do creator cung cấp có thể được giữ nguyên thay vì bị rewrite không cần thiết.

### Scene planning

Segment canonical được tạo trước SceneIntent. Scene plan giữ ổn định segment identity và narration linkage để về sau thay visual hoặc voice chính xác từng phần.

### Visuals

Visual route đi theo thứ tự đã resolve từ Studio Pack. Stock route dùng preview-first discovery và selection trước khi tải media full-size. Generated/stick route tuân theo approval policy của automation level.

Bạn có thể trộn visual automatic và manual theo từng scene.

### Voice

Voice được theo dõi theo từng segment. Audio/timing của segment đã verify có thể reuse. Thay một segment không buộc phải làm lại toàn bộ narration.

### Production Pack

Production Pack chỉ được assemble từ Content, SceneIntent, Visual, narration audio và timing đã chọn và verify. Voice timing là timeline clock ổn định cho duration và subtitle offset.

## 9. Universal Manual Takeover

Manual input không phải state song song hay workaround. Kết quả manual hợp lệ đi qua validation, canonical Job/Attempt với provenance, ArtifactStore promotion, physical verification rồi mới mở downstream DAG.

### 9.1 Tự cung cấp Script

Khi không muốn hoặc không thể tạo Content tự động, dùng:

- **Provide Script Manually**
- **Import TXT/Markdown**

Script được ingest thành canonical Content. Hệ thống không giả vờ rằng LLM provider đã chạy thành công.

Dùng flow này khi:

- LLMGateway offline/chưa cấu hình
- bạn đã viết script ở nơi khác
- muốn giữ nguyên wording do creator viết

### 9.2 Tự tạo Scene Plan

Sau khi Content đã thành công, dùng **Edit Scene Plan Manually**. Editor hỗ trợ:

- **Use Manual Scene Plan**
- **Import ScenePlan JSON**

Import/editor sẽ derive hoặc validate canonical ID, schema, linkage, narration và hash. Manual Script + Manual ScenePlan đủ để mở Visual stage mà không cần LLMGateway.

### 9.3 Tự cung cấp Visual theo từng scene

Ở bất kỳ scene nào, bạn có thể bỏ qua stock/generated provider và dùng manual takeover. Các flow hiện có gồm:

- chọn asset từ Asset Library
- dùng image của bạn
- dùng video của bạn
- **Provide Manually**
- **Replace Existing Visual**

Media được copy vào controlled workspace, hash, inspect tối thiểu, verify, promote qua ArtifactStore và bind đúng scene. Thay một scene chỉ invalidation dependency cone liên quan.

### 9.4 Tự cung cấp Voice và Timing theo từng segment

Khi TTS/voice/GPU không khả dụng, tự cung cấp narration theo segment. Recovery path hỗ trợ manual audio và timing, kể cả thay kết quả voice đã có.

Có thể dùng audio phù hợp như WAV/MP3 cùng timing/SRT tương ứng. Timing được validate để bảo đảm non-negative, ordered, đúng duration và đúng segment linkage.

Bạn có thể trộn:

```text
Segment A: audio manual
Segment B: voice automatic
Segment C: audio cũ + timing được sửa thủ công
```

mà không phải tạo lại các segment không liên quan.

### 9.5 External handoff cho generated/compute

Khi request generated image hoặc voice/compute đã được chuẩn bị nhưng runtime/provider local không chạy được, OmniCreator có thể export request provider-neutral để thực thi ở bên ngoài.

Các action điển hình:

- Copy Request
- Export Request
- xem Details
- Provide External Result
- Replace External Result

Quy tắc quan trọng:

- request export không chứa secret, provider-private field hay absolute path machine-local
- request hash được kiểm tra lại trước khi import result
- result stale không được gắn vào Content/SceneIntent mới hơn
- result external được ghi đúng provenance manual/external
- attempt provider/ComputeProvider thất bại không bị biến thành success giả

## 10. Review Center và các action recovery

Review Center dựng blocker và recovery health từ canonical state. Bộ action hiện tại:

- **Retry**: chuẩn bị/requeue canonical Job thật sự retryable
- **Configure**: đi đến surface cấu hình tương ứng như LLMGateway/plugin/runtime
- **Choose Alternative**: chỉ xuất hiện khi có ít nhất hai candidate ready thỏa đúng capability và product semantics thực sự cần user chọn
- **Provide Manually**: mở manual takeover tương ứng
- **Replace Result**: repair/relink artifact đã chọn nhưng bị thiếu/invalid
- **Details**: xem stage/item identity, blocker classification, reason, source, health và action hợp lệ

Review Center chọn một primary action khuyến nghị thay vì nhồi mọi nút vào từng card.

Fallback deterministic của Studio Pack không được giả thành menu chọn provider. Capability semantic phải khớp chính xác. Ví dụ `generated_still` chung không tự động thay thế cho semantic route `stick_figure_visual`.

## 11. GPU Workbench và burst compute

GPU Workbench được thiết kế để biến GPU thành tài nguyên batch tạm thời thay vì dependency bắt buộc luôn online.

Flow nên dùng:

1. chọn một hoặc nhiều project bằng checkbox **GPU BATCH**
2. chuẩn bị và review hết các dependency không cần GPU ở local
3. kiểm tra queue GPU đã chuẩn bị và các blocker
4. chỉ connect ComputeProvider khi queue đủ đáng để dùng GPU time
5. chạy GPU work
6. sync verified output về canonical local state
7. nếu worker biến mất, chỉ retry phần còn lại/retryable

Advanced provider detail là machine-local. Portable config chỉ nên giữ tên/reference của environment variable, không lưu secret value.

Nếu GPU không khả dụng, dùng manual audio/timing hoặc external handoff khi phù hợp. Không coi GPU là điều kiện bắt buộc để hoàn thành project.

## 12. Asset Library

Asset Library cung cấp media canonical có metadata, tag, usage history và thông tin source reuse.

Dùng Asset Library để:

- reuse asset đã verify
- tránh download/generate lại media giống hệt
- chọn library asset cho scene khi manual takeover
- xem provenance/source identity
- nhận biết duplicate hoặc cùng provider asset đã dùng trước đó

Khi chọn asset từ library, artifact vẫn được bind vào scene target qua canonical state.

## 13. Production Pack và export sang DaVinci Resolve

Khi toàn bộ required input đã verify, mở **Production Status** hoặc **Export to Resolve**.

Production Pack panel có thể hiển thị:

- trạng thái Production Pack canonical hiện tại
- portable `ProductionPackV1` JSON để inspect
- Export / Regenerate controls
- logical package location
- cache-hit feedback
- Job/Attempt history
- diagnostics cho missing artifact/relink

Flow bình thường không yêu cầu tự sửa Production Pack JSON.

### Chuỗi build/export

```text
verified Content + Scenes + Visuals + Audio + Timing
  -> Production Pack
  -> Subtitle / Timeline / Source Report / Interchange
  -> Resolve export
```

Nếu mở Data Root trên máy khác, nên regenerate phần interchange có chứa path dựa trên Data Root binding mới thay vì tin absolute path cũ còn đúng.

## 14. Production Recovery và relink

Nếu selected media bị thiếu hoặc hash-invalid, Production Recovery xác định vấn đề từ canonical artifact state. Recovery hỗ trợ repair/relink Visual, Audio, Timing hoặc Audio + Timing, sau đó rebuild Production Pack và regenerate Resolve output.

Automatic, manual và external artifact đều dùng chung recovery path canonical.

Khi thay artifact lỗi, chỉ dependency cone liên quan nên bị invalidated. Verified work không liên quan phải tiếp tục được reuse.

## 15. Restart, Resume, Retry và Cache

OmniCreator được thiết kế để tiếp tục từ persisted state thay vì chạy lại toàn bộ.

Sau restart:

- Project Kanban được dựng lại
- Job/Attempt history vẫn còn
- interrupted work được reconcile theo canonical state
- verified artifact có thể reuse
- failed/retryable work vẫn actionable
- Production Pack history có thể reload

Không xóa Data Root hoặc SQLite nội bộ để xử lý blocker thông thường. Hãy dùng Review Center, Retry, Replace Result, Production Recovery hoặc device handoff chuẩn.

## 16. Di chuyển Data Root hoặc chuyển sang máy khác

Flow an toàn:

1. hoàn thành hoặc pause công việc trên Machine A
2. dùng **Prepare for Device Handoff** hoặc đóng OmniCreator sạch
3. chờ Data Root copy/sync hoàn tất
4. trên Machine B, mở OmniCreator và chọn **Use Existing Data Folder**
5. chờ validate workspace/state/artifact
6. cài plugin machine-local còn thiếu nếu muốn dùng chúng
7. cấu hình credential còn thiếu nếu muốn dùng provider tự động tương ứng
8. tiếp tục project cũ

Với cloud-synced folder, không dùng hai writer cùng lúc. File sync không phải distributed database.

Nếu media đang ở trạng thái online-only, nên tải về/đánh dấu available offline trước khi mở DaVinci để tránh playback stall.

### Những gì đi cùng Data Root

Bao gồm project definition, WorkflowStep/Job/Attempt history, media/artifacts, voice takes, captions/timing, Studio Packs, provenance, export source data và clean state backup.

### Những gì cần cấu hình lại theo máy

Bao gồm plaintext secret, OS Keychain, disposable cache, runtime binary/environment, đường dẫn DaVinci application và per-device GPU preference.

## 17. Full manual workflow

OmniCreator đã được verify cho scenario hoàn thành project mà không cần LLM, stock provider, generated-image provider, voice provider, GPU hoặc external service khác.

Một flow hoàn toàn thủ công:

```text
Tạo project với workflow/Studio Pack phù hợp
  -> Provide Script Manually hoặc Import TXT/Markdown
  -> Edit / Import Manual Scene Plan
  -> Cung cấp image/video cho từng scene
  -> Cung cấp narration audio cho từng segment
  -> Cung cấp hoặc sửa timing
  -> Assemble/Rebuild Production Pack
  -> Export to Resolve
```

Dù input hoàn toàn manual, output vẫn đi qua canonical validation, provenance, ArtifactStore promotion, Job/Attempt history, dependency invalidation, restart/resume và logical URI portable.

## 18. Ví dụ mixed workflow

### Ví dụ A: LLM tạo content, visual làm thủ công

Dùng LLMGateway cho Content và Scenes, sau đó dùng image/video của bạn cho một số hoặc toàn bộ scene. Voice vẫn có thể chạy tự động rồi export.

### Ví dụ B: Script manual, stock automatic

Import script hoàn chỉnh, tự tạo/import ScenePlan, sau đó cho Pexels/Pixabay/Unsplash/Storyblocks tìm stock candidate.

### Ví dụ C: Visual automatic, voice manual

Cho Content, Scenes và Visuals chạy tự động, sau đó import narration và timing của bạn theo từng segment.

### Ví dụ D: Generated image chạy bên ngoài

Prepare generated-visual request, chạy request bằng tool/service ngoài phù hợp, import đúng result theo hash, sau đó tiếp tục Voice và Production Pack.

## 19. Troubleshooting

| Hiện tượng | Cách xử lý nên dùng |
| --- | --- |
| LLMGateway unavailable | Configure LLMGateway, Retry, hoặc tự cung cấp Script + ScenePlan |
| Studio Pack unavailable | Mở diagnostics / Plugin Manager và kiểm tra exact capability bị thiếu |
| Thiếu stock API key | Cấu hình environment variable machine-local hoặc dùng visual manual |
| Storyblocks search được nhưng không promote asset | Kiểm tra licensed production credential và `STORYBLOCKS_API_MODE=production` |
| Generated provider unavailable | Dùng image manual, Asset Library, route hợp lệ khác hoặc external handoff |
| Voice/GPU unavailable | Import audio + timing manual theo segment hoặc external handoff nếu đã có prepared request |
| Một visual/audio bị missing hoặc invalid | Dùng Review Center `Replace Result` / Production Recovery relink đúng item |
| Project đứng sau failure | Mở Review Center, xem Details rồi Retry/Configure/Provide Manually tùy blocker |
| Workspace báo có writer khác | Open Read Only hoặc đóng máy còn lại, chờ sync và Check Again |
| Chuyển Data Root xong Resolve còn path cũ | Regenerate Production Pack / Resolve export theo binding mới |
| Cloud file tồn tại nhưng chưa có local bytes | Hydrate/download file trước, tránh regenerate đắt tiền không cần thiết |

## 20. Quy tắc an toàn và toàn vẹn dữ liệu

Để project ổn định:

- chỉ dùng một writer cho mỗi Data Root
- không sửa canonical SQLite bằng tay
- không tự ép WorkflowStep thành SUCCEEDED
- không lưu API key/token/cookie vào portable project data
- không import external result stale sau khi Content/SceneIntent đã thay đổi
- dùng action replace/relink của ứng dụng thay vì thay file sau lưng ArtifactStore
- manual/external input phải đi qua validation path hỗ trợ
- đóng ứng dụng sạch trước khi đổi máy
- chờ cloud synchronization hoàn tất rồi mới mở workspace ở máy khác

## 21. Quick reference cho credential provider

```text
Pexels
  PEXELS_API_KEY

Pixabay
  PIXABAY_API_KEY

Unsplash
  UNSPLASH_ACCESS_KEY

Storyblocks
  STORYBLOCKS_PUBLIC_KEY
  STORYBLOCKS_PRIVATE_KEY
  STORYBLOCKS_USER_ID
  STORYBLOCKS_API_MODE=production   # cần để promote production download

Generated Image API (cấu hình mặc định)
  OPENAI_API_KEY
```

Tên environment variable có thể thay đổi trong advanced plugin settings nếu schema của plugin cho phép. Secret value vẫn phải nằm machine-local.

## 22. Tài liệu kỹ thuật liên quan

Nếu cần đi sâu vào implementation/architecture, xem:

- `docs/00-product-vision.md`
- `docs/01-system-architecture.md`
- `docs/02-plugin-architecture.md`
- `docs/03-scene-intelligence.md`
- `docs/04-kaggle-burst-compute.md`
- `docs/05-state-resume-retry.md`
- `docs/06-domain-model-ir.md`
- `docs/07-ux-studio-packs.md`
- `docs/08-davinci-integration.md`
- `docs/09-plugin-api-v1.md`
- `docs/10-roadmap.md`
- `docs/11-architecture-decisions.md`
- `docs/12-portable-data-root.md`
- `docs/13-review-center-recovery.md`

Đối với sử dụng hằng ngày, tài liệu này kết hợp với Review Center là đủ cho phần lớn tình huống. Các tài liệu kiến trúc hữu ích khi debug integration, phát triển plugin, kiểm tra portability hoặc xác minh workflow contract.
