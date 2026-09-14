use std::{
    env,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
};

use omnicreator_core::{
    evaluate_quality_with_provider_v1, CreatorContentSceneOptionsV1, CreatorInputV1,
    CreatorLlmExecutorV1, CreatorQualityOptions, Error, OpenAiCompatibleConfigV1,
    OpenAiCompatibleProviderV1, SegmentV1, VoiceDirectionV1, DEFAULT_OPENROUTER_API_KEY_ENV_V1,
    DEFAULT_OPENROUTER_MODEL_V1, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION, SEGMENT_SCHEMA,
    SEGMENT_SCHEMA_VERSION,
};
use serde::Deserialize;
use serde_json::json;

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4_096];
    loop {
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if bytes.len() >= header_end + 4 + content_length {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

fn write_chat_response(stream: &mut TcpStream, content: &str) {
    let body = serde_json::to_string(&json!({
        "id": "chat-test",
        "model": DEFAULT_OPENROUTER_MODEL_V1,
        "choices": [{"message": {"role": "assistant", "content": content}}],
    }))
    .unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
}

#[derive(Debug, Deserialize)]
struct QualityVerdict {
    score: u8,
    decision: String,
}

#[test]
fn direct_openrouter_profile_runs_content_scene_and_quality_without_llmgateway_semantics() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let server_capture = Arc::clone(&captured);
    let segment_text = "Morning light enters a quiet room as hope returns.";
    let scene_json = json!({
        "schema": SCENE_INTENT_SCHEMA,
        "schema_version": SCENE_INTENT_SCHEMA_VERSION,
        "id": "scene-1",
        "segment_id": "segment-1",
        "narration": segment_text,
        "purpose": "Show a concrete transition from uncertainty to hope.",
        "scene_type": "conceptual",
        "emotion_before": "uncertain",
        "emotion_after": "hopeful",
        "duration_hint": 6.0,
        "visual_ideas": [
            "Closed curtains slowly opening to morning light",
            "A quiet room becoming brighter as a person stands by the window"
        ],
        "search_queries": [
            "morning light through curtains interior",
            "person opening curtains sunrise room",
            "quiet bedroom warm dawn window"
        ],
        "avoid": [],
        "continuity": {},
        "aspect_ratio": "16:9"
    })
    .to_string();
    let responses = vec![
        "A calm production-ready narration script.".to_owned(),
        scene_json,
        r#"{"score":92,"decision":"pass"}"#.to_owned(),
    ];

    let server = thread::spawn(move || {
        for content in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            server_capture.lock().unwrap().push(request);
            write_chat_response(&mut stream, &content);
        }
    });

    let provider = OpenAiCompatibleProviderV1::new(OpenAiCompatibleConfigV1 {
        base_url: format!("http://{address}/api/v1"),
        ..OpenAiCompatibleConfigV1::openrouter_v1()
    })
    .unwrap();

    let previous = env::var(DEFAULT_OPENROUTER_API_KEY_ENV_V1).ok();
    env::set_var(DEFAULT_OPENROUTER_API_KEY_ENV_V1, "test-secret");

    let script = provider
        .create_script_v1(&CreatorInputV1::topic("How hope grows after uncertainty"))
        .unwrap();
    assert_eq!(script, "A calm production-ready narration script.");

    let segment = SegmentV1 {
        schema: SEGMENT_SCHEMA.to_owned(),
        schema_version: SEGMENT_SCHEMA_VERSION,
        id: "segment-1".to_owned(),
        order: 1,
        text: segment_text.to_owned(),
        voice_direction: VoiceDirectionV1::default(),
    };
    let scene = provider
        .create_scene_intent_v1(
            &segment,
            "scene-1",
            &CreatorContentSceneOptionsV1::default(),
        )
        .unwrap();
    assert_eq!(scene.id, "scene-1");
    assert_eq!(scene.segment_id, "segment-1");
    assert_eq!(scene.narration, segment_text);

    let quality: QualityVerdict = evaluate_quality_with_provider_v1(
        &provider,
        "quality-verdict-v1",
        "Pass only when the narration is clear and production-ready.",
        &script,
        &CreatorQualityOptions {
            max_attempts: 1,
            ..CreatorQualityOptions::default()
        },
        |verdict: &QualityVerdict| {
            if verdict.score >= 80 && verdict.decision == "pass" {
                Ok(())
            } else {
                Err(Error::InvalidContract(
                    "quality verdict did not pass".to_owned(),
                ))
            }
        },
    )
    .unwrap();
    assert_eq!(quality.score, 92);
    assert_eq!(quality.decision, "pass");

    if let Some(previous) = previous {
        env::set_var(DEFAULT_OPENROUTER_API_KEY_ENV_V1, previous);
    } else {
        env::remove_var(DEFAULT_OPENROUTER_API_KEY_ENV_V1);
    }

    server.join().unwrap();
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 3);
    for request in requests.iter() {
        assert!(request.starts_with("POST /api/v1/chat/completions "));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-secret"));
        assert!(request.contains("\"model\":\"openrouter/auto\""));
        assert!(!request.contains("llmgateway_task"));
        assert!(!request.contains("/_llmgateway/"));
    }
}
