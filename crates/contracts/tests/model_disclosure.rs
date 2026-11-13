use edgeagent_contracts::{
    evidence::{EvidenceDigest, EvidenceId},
    explanation::{Anchor, ServingPermit},
    explanation_profile::Profile,
    model_disclosure::{MAX_PERMIT_BYTES, MAX_REQUEST_BYTES, Permit, Purpose, Request},
    research::ResearchRecord,
};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn request() -> Result<Request> {
    let ResearchRecord::Request(research) =
        ResearchRecord::decode(include_bytes!("../fixtures/research/v1/request.json"))?
    else {
        return Err("request".into());
    };
    let profile: Profile = serde_json::from_value(
        serde_json::json!({"deployment":{"runtime":"a".repeat(64),"weights":"b".repeat(64)},"settings":{"seed":1,"temperature_milli":0,"max_output_tokens":512,"timeout_ms":1000},"prompt":"c".repeat(64),"validator":"d".repeat(64),"renderer":"e".repeat(64),"tools":"none"}),
    )?;
    Ok(Request {
        operation: EvidenceId::new(&"1".repeat(32))?,
        anchor: Anchor {
            scope: research.scope.clone(),
            artifact: "a".repeat(64),
            decision: "b".repeat(64),
        },
        research,
        acquisition: EvidenceId::new(&"2".repeat(32))?,
        publication: "c".repeat(64),
        evidence: EvidenceDigest::new(&"d".repeat(64))?,
        audience: "local-model".into(),
        purpose: Purpose::ModelDisclosure,
        profile,
        input_contract: "e".repeat(64),
        input: "f".repeat(64),
    })
}
fn permit() -> Result<Permit> {
    let request = request()?;
    let rights_scope = "9".repeat(64);
    let review_key = request.review_key(&rights_scope)?;
    Ok(Permit {
        request,
        rights_scope,
        review_key,
        review_revision: 1,
        agreement: EvidenceId::new(&"3".repeat(32))?,
        checked_at_ms: 100,
        expires_at_ms: 105,
        review_until_ms: 110,
        retention_until_ms: 107,
    })
}
#[test]
fn canonical_codecs_and_snapshot_bounds_are_strict() -> Result {
    let request = request()?;
    let bytes = request.canonical_bytes()?;
    assert!(Request::decode(&bytes)?.canonical_bytes()? == bytes);
    let permit = permit()?;
    let wire = permit.canonical_bytes()?;
    assert!(Permit::decode(&wire)?.canonical_bytes()? == wire);
    permit.bind(&request, 100)?;
    permit.bind(&request, 104)?;
    assert!(permit.bind(&request, 99).is_err());
    assert!(permit.bind(&request, 105).is_err());
    assert!(Request::decode(&vec![b' '; MAX_REQUEST_BYTES + 1]).is_err());
    assert!(Permit::decode(&vec![b' '; MAX_PERMIT_BYTES + 1]).is_err());
    let permit_text = String::from_utf8(wire)?;
    for invalid in [
        permit_text.replace("permit.v1", "permit.v2"),
        permit_text.replace(
            "\"review_revision\":",
            "\"unknown\":true,\"review_revision\":",
        ),
        format!(" {permit_text}"),
    ] {
        assert!(Permit::decode(invalid.as_bytes()).is_err());
    }
    let text = String::from_utf8(bytes)?;
    for invalid in [
        text.replace("request.v1", "request.v2"),
        text.replace("\"model_disclosure\"", "\"explanation_serving\""),
        text.replace("\"audience\":", "\"allow\":true,\"audience\":"),
        text.replace("\"audience\":", "\"audience\":\"other\",\"audience\":"),
        text.replace(&"e".repeat(64), &"0".repeat(64)),
        text.replace("local-model", &"x".repeat(65)),
        format!(" {text}"),
    ] {
        assert!(Request::decode(invalid.as_bytes()).is_err());
    }
    assert!(
        Permit::decode(&serde_json::to_vec(&ServingPermit {
            scope: "offline".into(),
            acquisition: "2".repeat(32),
            evidence: "d".repeat(64),
            audience: "reader".into(),
            checked_at_ms: 100,
            expires_at_ms: 105
        })?)
        .is_err()
    );
    {
        let mut changed = permit.clone();
        changed.expires_at_ms = 108;
        assert!(changed.canonical_bytes().is_err());
    }
    let mut expired = permit.clone();
    expired.expires_at_ms = 100;
    assert!(expired.canonical_bytes().is_err());
    let mut review = permit.clone();
    review.review_until_ms = 104;
    assert!(review.canonical_bytes().is_err());
    let mut early = permit.clone();
    early.checked_at_ms = 99;
    assert!(early.canonical_bytes().is_err());
    let mut stale = permit.clone();
    stale.review_revision = 0;
    assert!(stale.canonical_bytes().is_err());
    Ok(())
}
#[test]
fn every_dispatch_subject_and_recipient_field_is_bound() -> Result {
    let permit = permit()?;
    for field in 0..12 {
        let mut expected = permit.request.clone();
        match field {
            0 => expected.operation = EvidenceId::new(&"4".repeat(32))?,
            1 => expected.acquisition = EvidenceId::new(&"4".repeat(32))?,
            2 => expected.publication = "4".repeat(64),
            3 => expected.evidence = EvidenceDigest::new(&"4".repeat(64))?,
            4 => expected.input = "4".repeat(64),
            5 => expected.input_contract = "4".repeat(64),
            6 => expected.audience = "other-model".into(),
            7 => expected.profile.deployment.weights = "4".repeat(64),
            8 => expected.profile.deployment.runtime = "4".repeat(64),
            9 => expected.profile.settings.seed = 2,
            10 => expected.anchor.decision = "4".repeat(64),
            _ => expected.research.request_id = "other-request".into(),
        }
        assert!(
            permit.bind(&expected, 100).is_err(),
            "substituted field {field}"
        );
    }
    Ok(())
}
#[test]
fn review_key_is_scoped_to_rights_recipient_and_contract_not_payload_instances() -> Result {
    let original = request()?;
    let scope = "9".repeat(64);
    let key = original.review_key(&scope)?;
    assert_eq!(
        key,
        "76147fdd24a8d92a3f044c88d082e1c59f213c27607d65e42684cbb9856cab3a"
    );
    let mut instance = original.clone();
    instance.operation = EvidenceId::new(&"4".repeat(32))?;
    instance.input = "4".repeat(64);
    instance.acquisition = EvidenceId::new(&"4".repeat(32))?;
    assert_eq!(key, instance.review_key(&scope)?);
    assert_ne!(key, original.review_key(&"8".repeat(64))?);
    for field in 0..5 {
        let mut changed = original.clone();
        match field {
            0 => changed.profile.deployment.runtime = "4".repeat(64),
            1 => changed.profile.deployment.weights = "4".repeat(64),
            2 => changed.profile.prompt = "4".repeat(64),
            3 => changed.audience = "other".into(),
            _ => changed.input_contract = "4".repeat(64),
        }
        assert_ne!(key, changed.review_key(&scope)?);
    }
    let mut forged = permit()?;
    forged.rights_scope = "8".repeat(64);
    assert!(forged.canonical_bytes().is_err());
    let mut foreign = original.clone();
    foreign.anchor.scope = "foreign".into();
    assert!(foreign.canonical_bytes().is_err());
    Ok(())
}
