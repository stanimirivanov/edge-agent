use edgeagent_contracts::research_controls::{Command, MAX_CONTROL_BYTES, Target};
type Result = std::result::Result<(), Box<dyn std::error::Error>>;
#[test]
fn control_commands_are_bounded_canonical_and_never_accept_actor_or_time() -> Result {
    let record = Command::V1 {
        scope: "operator".into(),
        target: Target::Scope,
        revision: 1,
        enabled: false,
        reason_code: "maintenance".into(),
    };
    let bytes = record.bytes()?;
    assert_eq!(bytes,b"{\"data\":{\"enabled\":false,\"reason_code\":\"maintenance\",\"revision\":1,\"scope\":\"operator\",\"target\":{\"kind\":\"scope\"}},\"kind\":\"research.control.v1\"}\n");
    assert!(Command::decode(&bytes)? == record);
    for field in 0..8 {
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
        match field {
            0 => value["data"]["actor"] = "PRIVATE_SENTINEL".into(),
            1 => value["data"]["occurred_at"] = 0.into(),
            2 => value["data"]["revision"] = 0.into(),
            3 => value["data"]["revision"] = u64::MAX.into(),
            4 => {
                value["data"]["target"] =
                    serde_json::json!({"kind":"implementation","digest":"0".repeat(64)})
            }
            5 => value["data"]["reason_code"] = "private free text".into(),
            6 => value["kind"] = "research.control.v2".into(),
            _ => {
                value["data"]
                    .as_object_mut()
                    .ok_or("object")?
                    .remove("enabled");
            }
        }
        let mut invalid = serde_json::to_vec(&value)?;
        invalid.push(b'\n');
        let error = Command::decode(&invalid).err().ok_or("accepted invalid")?;
        assert!(!format!("{error:?} {error}").contains("PRIVATE_SENTINEL"));
    }
    let duplicate = String::from_utf8(bytes.clone())?
        .replace("\"revision\":1", "\"revision\":1,\"revision\":1");
    assert!(Command::decode(duplicate.as_bytes()).is_err());
    assert!(Command::decode(&bytes[..bytes.len() - 1]).is_err());
    assert!(Command::decode(&vec![0; MAX_CONTROL_BYTES + 1]).is_err());
    let implementation = Command::V1 {
        scope: "operator".into(),
        target: Target::Implementation {
            digest: "02".repeat(32),
        },
        revision: i64::MAX as u64,
        enabled: true,
        reason_code: "reviewed-resume".into(),
    };
    assert!(Command::decode(&implementation.bytes()?)? == implementation);
    Ok(())
}
