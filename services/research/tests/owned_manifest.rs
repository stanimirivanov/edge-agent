use edgeagent_research::explanation_owned::Manifest;
#[test]
fn rejects_historical_unbounded_or_foreign_manifests() {
    for bytes in [b"{}".as_slice(), b"{\"version\":1}", &[b' '; 32769]] {
        assert!(Manifest::decode(bytes).is_err());
    }
}

#[test]
fn rust_and_supervisor_bind_the_same_files_hardware_and_deployment()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use edgeagent_contracts::explanation_profile::{Deployment, Profile, Settings, Tools};
    use edgeagent_research::explanation_owned::digest;
    use serde_json::json;
    use std::{fs, process::Command};
    let root = std::env::temp_dir().join(format!("owned-manifest-{}", std::process::id()));
    fs::create_dir(&root)?;
    let result = (|| {
        let executable = std::env::current_exe()?;
        for name in ["runtime", "weights", "license", "prompt"] {
            fs::write(root.join(name), name)?;
        }
        let pin = |path: &std::path::Path| -> Result<serde_json::Value, std::io::Error> {
            Ok(json!({"path":path,"sha256":digest(&fs::read(path)?)}))
        };
        let mut value = json!({"version":2,"runtime":pin(&root.join("runtime"))?,"weights":pin(&root.join("weights"))?,
            "license":pin(&root.join("license"))?,"helper":pin(&executable)?,"prompt":pin(&root.join("prompt"))?,
            "profile":{"path":root.join("profile"),"sha256":"a".repeat(64)},"libraries":[],"threads":1,"context":4096,"hardware":{}});
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
        let code = "import json,sys; sys.path.insert(0,sys.argv[1]); import run_explanation_owned as x; m=json.load(sys.stdin); m['hardware']=x.common.hardware(); print(json.dumps({'hardware':m['hardware'],'runtime':x.runtime_id(m)}))";
        let mut child = Command::new("python3")
            .args(["-c", code, script.to_str().ok_or("scripts")?])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        use std::io::Write as _;
        child
            .stdin
            .take()
            .ok_or("stdin")?
            .write_all(&serde_json::to_vec(&value)?)?;
        let output = child.wait_with_output()?;
        assert!(output.status.success());
        let identity: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        value["hardware"] = identity["hardware"].clone();
        let profile = Profile {
            deployment: Deployment {
                runtime: identity["runtime"].as_str().ok_or("runtime")?.into(),
                weights: value["weights"]["sha256"].as_str().ok_or("weights")?.into(),
            },
            settings: Settings {
                seed: 1,
                temperature_milli: 0,
                max_output_tokens: 512,
                timeout_ms: 1000,
            },
            prompt: digest(b"prompt"),
            validator: digest(b"research-explanation-allowlist-v1"),
            renderer: digest(b"research-explanation-runtime.v1"),
            tools: Tools::None,
        };
        let bytes = profile.canonical_bytes()?;
        fs::write(root.join("profile"), &bytes)?;
        value["profile"]["sha256"] = digest(&bytes).into();
        let manifest = Manifest::decode(&serde_json::to_vec(&value)?)?;
        assert_eq!(manifest.profile()?.id()?, profile.id()?);
        assert_eq!(manifest.verify(&profile)?, "prompt");
        fs::write(root.join("weights"), "changed")?;
        assert!(manifest.verify(&profile).is_err());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })();
    fs::remove_dir_all(root)?;
    result
}

#[test]
fn cuda_manifest_is_explicit_and_command_is_closed()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use serde_json::json;
    let pin = json!({"path":"/invented/file","sha256":"a".repeat(64)});
    let mut value = json!({"version":3,"runtime":pin,"weights":pin,"license":pin,"helper":pin,"prompt":pin,"profile":pin,"libraries":[],"threads":6,"context":32768,"hardware":{},
        "cuda":{"pci":"0000:01:00.0","uuid":"GPU-11111111-1111-1111-1111-111111111111","driver":"a".repeat(64),"layers":32,"cache":"q4_0"}});
    let manifest = Manifest::decode(&serde_json::to_vec(&value)?)?;
    let command = manifest.command(1234);
    assert!(command.windows(2).any(|v| v == ["--device", "CUDA0"]));
    assert!(command.windows(2).any(|v| v == ["--n-gpu-layers", "32"]));
    assert!(command.windows(2).any(|v| v == ["--cache-type-v", "q4_0"]));
    value["version"] = 2.into();
    assert!(Manifest::decode(&serde_json::to_vec(&value)?).is_err());
    let descriptor = value["cuda"].take();
    assert!(Manifest::decode(&serde_json::to_vec(&value)?).is_err());
    value["cuda"] = descriptor;
    value["version"] = 3.into();
    value["cuda"]["cache"] = "arbitrary".into();
    assert!(Manifest::decode(&serde_json::to_vec(&value)?).is_err());
    value["cuda"]["cache"] = "q4_0".into();
    value["cuda"]["pci"] = "../../other".into();
    assert!(Manifest::decode(&serde_json::to_vec(&value)?).is_err());
    Ok(())
}
