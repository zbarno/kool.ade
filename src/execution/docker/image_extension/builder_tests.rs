use super::*;
fn request() -> AuthorizedImageExtensionRequest {
    AuthorizedImageExtensionRequest {
        task_id: "task-1".into(),
        project_id: "project-1".into(),
        base_reference: "registry.example/executor@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        base_digest: format!("sha256:{}", "a".repeat(64)),
        base_profile: crate::execution::docker::images::RuntimeProfile::General,
        target_platform: "linux/amd64".into(),
        requested_packages: vec!["curl".into()],
        package_manifest: vec![artifact(b"synthetic deb").identity],
        purpose: "Install required system tool".into(),
        request_identity: "sha256:request".into(),
    }
}

fn artifact(bytes: &[u8]) -> VerifiedPackageArtifact {
    VerifiedPackageArtifact {
        identity: PackageArtifactIdentity {
            name: "curl".into(),
            version: "7.88.1-10+deb12u14".into(),
            architecture: "amd64".into(),
            source: "https://deb.debian.org/debian/pool/main/c/curl/curl_7.88.1_amd64.deb".into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        },
        bytes: bytes.to_vec(),
    }
}

#[test]
fn generated_context_pins_base_and_contains_only_verified_artifacts() {
    let context = generate_context(&request(), vec![artifact(b"synthetic deb")]).unwrap();
    let mut archive = tar::Archive::new(Cursor::new(&context.tar));
    let entries = archive
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
            (path, bytes)
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);
    let dockerfile = String::from_utf8(entries[0].1.clone()).unwrap();
    assert!(dockerfile.contains(&request().base_reference));
    assert!(dockerfile.contains("RUN --network=none"));
    assert!(dockerfile.contains("dpkg --force-confold -i"));
    assert_eq!(
        entries[1],
        ("packages/0000.deb".into(), b"synthetic deb".to_vec())
    );
    assert!(context.content_identity.starts_with("sha256:"));
    assert_eq!(context.package_manifest[0].version, "7.88.1-10+deb12u14");
}

#[test]
fn context_rejects_tampered_bytes_and_untrusted_source_paths() {
    let mut tampered = artifact(b"synthetic deb");
    tampered.bytes.push(0);
    assert!(generate_context(&request(), vec![tampered]).is_err());
    let mut untrusted = artifact(b"synthetic deb");
    untrusted.identity.source = "https://deb.debian.org/debian/pool/main/../evil.deb".into();
    assert!(generate_context(&request(), vec![untrusted]).is_err());
}

#[test]
fn installed_manifest_must_match_exact_package_versions() {
    let expected = vec![artifact(b"synthetic deb").identity];
    assert!(verify_installed_manifest("curl=7.88.1-10+deb12u14=amd64\n", &expected).is_ok());
    assert!(verify_installed_manifest("curl=7.88.1-10+deb12u13=amd64\n", &expected).is_err());
    assert!(verify_installed_manifest("curl=7.88.1-10+deb12u14=arm64\n", &expected).is_err());
    assert!(
        verify_installed_manifest(
            "curl=7.88.1-10+deb12u14=amd64\nlibcurl4=7.88.1=amd64\n",
            &expected
        )
        .is_err()
    );
}
