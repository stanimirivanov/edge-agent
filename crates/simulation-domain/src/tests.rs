use super::*;
use edgeagent_market_data_domain::PreciseInstant;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn time(seconds: i64) -> Result<PreciseInstant, Box<dyn std::error::Error>> {
    Ok(PreciseInstant::from_unix_parts(seconds, 0)?)
}
fn input(
    feed: u128,
    sequence: u64,
    seconds: i64,
) -> Result<FrozenInput, Box<dyn std::error::Error>> {
    Ok(FrozenInput {
        available: time(seconds)?,
        feed,
        epoch: 1,
        sequence: Some(sequence),
        identity: (u128::from(sequence) + 1, 1),
        provenance: [1; 9],
        artifact: ArtifactDigest::of(&sequence.to_be_bytes()),
    })
}
fn manifest(inputs: &[FrozenInput]) -> Result<RunManifest, RunError> {
    RunManifest::new(
        inputs.first().ok_or(RunError::Input)?.available,
        ArtifactDigest::of(b"implementation"),
        ArtifactDigest::of(b"configuration"),
        [0; 32],
        inputs,
    )
}
fn outputs(run: &mut Run<'_>) -> Result<Vec<(DeterministicId, u64, PreciseInstant)>, RunError> {
    let mut result = Vec::new();
    while run.next_input().is_some() {
        result.push((run.next_id()?, run.random_u64()?, run.now()));
    }
    Ok(result)
}

#[test]
fn repeat_reorder_duplicate_and_restart_have_identical_results() -> TestResult {
    let inputs = [
        input(2, 1, 10)?,
        input(1, 2, 10)?,
        input(1, 1, 10)?,
        input(1, 3, 11)?,
    ];
    let original = manifest(&inputs)?;
    let shuffled = manifest(&[
        inputs[1].clone(),
        inputs[3].clone(),
        inputs[2].clone(),
        inputs[0].clone(),
        inputs[0].clone(),
    ])?;
    assert_eq!(original.canonical_bytes(), shuffled.canonical_bytes());
    assert_eq!(
        original
            .inputs()
            .iter()
            .map(FrozenInput::identity)
            .collect::<Vec<_>>(),
        vec![(2, 1), (3, 1), (2, 1), (4, 1)]
    );
    let expected = outputs(&mut Run::new(&original))?;
    assert_eq!(outputs(&mut Run::new(&shuffled))?, expected);
    for committed in 0..=inputs.len() {
        let mut run = Run::new(&original);
        let mut actual = Vec::new();
        for _ in 0..committed {
            assert!(run.next_input().is_some());
            actual.push((run.next_id()?, run.random_u64()?, run.now()));
        }
        let checkpoint = run.checkpoint();
        let mut restored = Run::restore(&original, &checkpoint)?;
        actual.extend(outputs(&mut restored)?);
        assert_eq!(actual, expected);
        assert!(restored.next_input().is_none());
    }
    Ok(())
}

#[test]
fn conflicting_identity_sequence_and_inverted_source_order_fail_closed() -> TestResult {
    let first = input(1, 1, 10)?;
    let mut conflict = first.clone();
    conflict.artifact = ArtifactDigest::of(b"conflict");
    assert_eq!(
        manifest(&[first.clone(), conflict]).err(),
        Some(RunError::Conflict)
    );
    let mut conflict = first.clone();
    conflict.identity.0 = 99;
    assert_eq!(
        manifest(&[first.clone(), conflict]).err(),
        Some(RunError::Conflict)
    );
    assert_eq!(
        manifest(&[input(1, 2, 10)?, input(1, 1, 11)?]).err(),
        Some(RunError::Conflict)
    );
    assert!(manifest(&[first, input(2, 1, 10)?]).is_ok());
    for field in 1..7 {
        let first = input(1, 1, 10)?;
        let mut switched = input(1, 2, 11)?;
        switched.provenance[field] = 2;
        assert_eq!(
            manifest(&[first.clone(), switched.clone()]).err(),
            Some(RunError::Conflict)
        );
        switched.epoch = 2;
        assert!(manifest(&[first, switched]).is_ok());
    }
    Ok(())
}

#[test]
fn clock_cannot_skip_inputs_and_rejections_leave_checkpoint_unchanged() -> TestResult {
    let manifest = manifest(&[input(1, 1, 10)?, input(1, 2, 12)?])?;
    let mut run = Run::new(&manifest);
    let before = run.checkpoint();
    assert_eq!(run.advance_to(time(11)?), Err(RunError::Clock));
    assert_eq!(run.advance_to(time(9)?), Err(RunError::Clock));
    assert_eq!(run.checkpoint(), before);
    assert_eq!(
        run.next_input().map(FrozenInput::available_at),
        Some(time(10)?)
    );
    run.advance_to(time(11)?)?;
    let checkpoint = run.checkpoint();
    let mut restored = Run::restore(&manifest, &checkpoint)?;
    assert_eq!(restored.now(), time(11)?);
    assert_eq!(
        restored.next_input().map(FrozenInput::available_at),
        Some(time(12)?)
    );
    assert!(restored.next_input().is_none());
    restored.advance_to(time(20)?)?;
    assert_eq!(
        Run::restore(&manifest, &restored.checkpoint())?.now(),
        time(20)?
    );
    Ok(())
}

#[test]
fn checkpoint_rejects_every_truncation_corruption_and_manifest_change() -> TestResult {
    let inputs = [input(1, 1, 10)?];
    let manifest = manifest(&inputs)?;
    let checkpoint = Run::new(&manifest).checkpoint();
    assert_eq!(checkpoint.len(), 108);
    for length in 0..checkpoint.len() {
        assert_eq!(
            Run::restore(&manifest, &checkpoint[..length]).err(),
            Some(RunError::Checkpoint)
        );
    }
    for offset in 0..checkpoint.len() {
        let mut changed = checkpoint.clone();
        changed[offset] ^= 1;
        assert_eq!(
            Run::restore(&manifest, &changed).err(),
            Some(RunError::Checkpoint)
        );
    }
    let mut extra = checkpoint.clone();
    extra.push(0);
    assert_eq!(
        Run::restore(&manifest, &extra).err(),
        Some(RunError::Checkpoint)
    );
    for changed in [
        RunManifest::new(
            time(10)?,
            ArtifactDigest::of(b"new-code"),
            ArtifactDigest::of(b"configuration"),
            [0; 32],
            &inputs,
        )?,
        RunManifest::new(
            time(10)?,
            ArtifactDigest::of(b"implementation"),
            ArtifactDigest::of(b"new-config"),
            [0; 32],
            &inputs,
        )?,
        RunManifest::new(
            time(10)?,
            ArtifactDigest::of(b"implementation"),
            ArtifactDigest::of(b"configuration"),
            [1; 32],
            &inputs,
        )?,
    ] {
        assert_ne!(changed.digest(), manifest.digest());
        assert_eq!(
            Run::restore(&changed, &checkpoint).err(),
            Some(RunError::Checkpoint)
        );
    }
    Ok(())
}

#[test]
fn limits_artifact_verification_and_diagnostics_are_explicit() -> TestResult {
    let first = input(1, 1, 10)?;
    assert_eq!(manifest(&[]).err(), Some(RunError::Input));
    assert_eq!(
        manifest(&vec![first.clone(); 4097]).err(),
        Some(RunError::Input)
    );
    assert!(manifest(&vec![first.clone(); 4096]).is_ok());
    let manifest = manifest(&[first])?;
    let mut run = Run::new(&manifest);
    // Independent ChaCha20 zero-key/zero-nonce first-block reference vector.
    assert_eq!(run.random_u64()?, 0x903d_f1a0_ade0_b876);
    let id = run.next_id()?;
    let mut other = Run::new(&manifest);
    assert_eq!(other.next_id()?, id); // RNG draws cannot perturb IDs.
    let bytes = b"LICENSED-SENTINEL";
    ArtifactDigest::of(bytes).verify(bytes)?;
    assert_eq!(
        ArtifactDigest::of(bytes).verify(b"wrong"),
        Err(RunError::Artifact)
    );
    let debug = format!("{manifest:?}{run:?}{:?}", ArtifactDigest::of(bytes));
    assert!(!debug.contains("LICENSED-SENTINEL"));
    assert!(!debug.contains("seed"));
    Ok(())
}

fn rewrite(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
    let digest = ArtifactDigest::of(&bytes[..76]).bytes();
    bytes[76..].copy_from_slice(&digest);
}

#[test]
fn valid_checksum_cannot_bypass_continuation_invariants_or_counter_limits() -> TestResult {
    let manifest = manifest(&[input(1, 1, 10)?, input(1, 2, 12)?])?;
    let initial = Run::new(&manifest).checkpoint();
    for (offset, value) in [
        (36, 3_u64),         // cursor exceeds input count
        (44, 9),             // before start
        (44, 11),            // skips pending availability
        (52, 1_000_000_000), // invalid nanoseconds
    ] {
        let mut bytes = initial.clone();
        rewrite(&mut bytes, offset, &value.to_be_bytes());
        assert_eq!(
            Run::restore(&manifest, &bytes).err(),
            Some(RunError::Checkpoint)
        );
    }
    let mut bytes = initial.clone();
    rewrite(&mut bytes, 0, b"EAC2");
    assert_eq!(
        Run::restore(&manifest, &bytes).err(),
        Some(RunError::Checkpoint)
    );
    // A consumed input cannot have a clock earlier than its availability.
    let mut bytes = initial.clone();
    rewrite(&mut bytes, 36, &2_u64.to_be_bytes());
    assert_eq!(
        Run::restore(&manifest, &bytes).err(),
        Some(RunError::Checkpoint)
    );
    for offset in [60, 68] {
        let mut bytes = initial.clone();
        rewrite(&mut bytes, offset, &(u64::MAX - 1).to_be_bytes());
        let mut run = Run::restore(&manifest, &bytes)?;
        if offset == 60 {
            run.next_id()?;
        } else {
            run.random_u64()?;
        }
        let before = run.checkpoint();
        if offset == 60 {
            assert_eq!(run.next_id().err(), Some(RunError::Exhausted));
        } else {
            assert_eq!(run.random_u64(), Err(RunError::Exhausted));
        }
        assert_eq!(run.checkpoint(), before);
    }
    Ok(())
}

#[test]
fn epoch_revision_and_start_conflicts_reject_without_reordering_history() -> TestResult {
    let first = input(1, 1, 10)?;
    let mut later = input(1, 2, 12)?;
    later.epoch = 2;
    let mut older = input(1, 3, 13)?;
    older.epoch = 1;
    assert_eq!(
        manifest(&[first.clone(), later, older]).err(),
        Some(RunError::Conflict)
    );
    let mut newer = first.clone();
    newer.sequence = None;
    newer.identity.1 = 2;
    let mut older = first.clone();
    older.sequence = None;
    older.available = time(12)?;
    assert_eq!(manifest(&[newer, older]).err(), Some(RunError::Conflict));
    assert_eq!(
        RunManifest::new(
            time(11)?,
            ArtifactDigest::of(b"code"),
            ArtifactDigest::of(b"config"),
            [0; 32],
            &[first]
        )
        .err(),
        Some(RunError::Input)
    );
    Ok(())
}

#[test]
fn canonical_manifest_and_ids_match_independent_sha256_vectors() -> TestResult {
    // Independently computed with Python hashlib and explicit big-endian widths.
    let manifest = manifest(&[input(1, 1, 10)?])?;
    assert_eq!(manifest.canonical_bytes().len(), 408);
    assert_eq!(
        manifest.digest().bytes(),
        [
            0xea, 0xeb, 0x3c, 0x0c, 0x0e, 0xa7, 0x98, 0x97, 0xb3, 0xe0, 0x80, 0xf6, 0x79, 0xa3,
            0xc5, 0x28, 0xdb, 0x8a, 0x54, 0x20, 0xa2, 0x6b, 0xb3, 0xfa, 0xba, 0xe4, 0x3c, 0x90,
            0x74, 0xda, 0x75, 0x3b,
        ]
    );
    assert_eq!(
        Run::new(&manifest).next_id()?.bytes(),
        [
            0xc3, 0x7b, 0xb4, 0x31, 0xd3, 0x29, 0x6a, 0xdc, 0x80, 0xbd, 0x98, 0x1c, 0x83, 0x68,
            0xdf, 0x67, 0x11, 0x45, 0x0d, 0x0c, 0xdf, 0x1e, 0x7a, 0x65, 0xf7, 0x1c, 0x1d, 0x4c,
            0x29, 0x18, 0x17, 0x4d,
        ]
    );
    Ok(())
}

#[test]
fn sequence_absence_zero_and_nanosecond_boundaries_remain_distinct() -> TestResult {
    let mut first = input(1, 0, 0)?;
    first.available = PreciseInstant::from_unix_parts(-1, 999_999_999)?;
    let mut missing = first.clone();
    missing.sequence = None;
    let sequenced = manifest(&[first.clone()])?;
    let snapshots = manifest(&[missing])?;
    assert_ne!(sequenced.digest(), snapshots.digest());
    let mut next = input(1, 1, 0)?;
    next.available = PreciseInstant::from_unix_parts(0, 0)?;
    let manifest = manifest(&[first.clone(), next])?;
    let mut run = Run::new(&manifest);
    assert_eq!(
        run.next_input().map(FrozenInput::available_at),
        Some(first.available)
    );
    let mut restored = Run::restore(&manifest, &run.checkpoint())?;
    assert_eq!(restored.now(), first.available);
    assert_eq!(
        restored.next_input().map(FrozenInput::available_at),
        Some(time(0)?)
    );
    Ok(())
}
