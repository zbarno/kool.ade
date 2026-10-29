use super::{collect, package_identities_from_contents};
use std::fs;

fn hash(value: char) -> String {
    value.to_string().repeat(64)
}

#[test]
fn reads_exact_hash_locked_requirements_and_normalizes_python_names() {
    let contents = format!(
        "# Generated lock\nColorama==0.4.6 --hash=sha256:{}\nzope_interface==5.4.0 --hash=sha256:{} --hash=sha256:{}\n",
        hash('a'),
        hash('b'),
        hash('c')
    );
    let packages = package_identities_from_contents(&contents).unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0].package, "colorama");
    assert_eq!(packages[0].version, "0.4.6");
    assert_eq!(packages[0].source, "https://pypi.org");
    assert_eq!(packages[0].integrity, format!("sha256:{}", hash('a')));
    assert_eq!(packages[1].package, "zope-interface");
    assert_eq!(
        packages[1].integrity,
        format!("sha256:{},{}", hash('b'), hash('c'))
    );
}

#[test]
fn rejects_unpinned_url_vcs_included_or_unhashed_requirements() {
    for line in [
        "requests>=2.0 --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "requests==2.0",
        "requests @ https://files.pythonhosted.org/packages/request.whl",
        "git+https://github.com/example/repo.git#egg=requests",
        "-r nested-requirements.txt",
        "--index-url https://attacker.example/simple",
        "requests==2.0; python_version > '3.10' --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "requests==2.0 --hash=sha1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "requests==2.0 --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa # comment",
        "../requests==2.0 --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert!(
            package_identities_from_contents(line).is_err(),
            "accepted {line:?}"
        );
    }
}

#[test]
fn rejects_duplicate_names_conflicting_versions_and_invalid_hashes() {
    let pin = format!("sample==1.0 --hash=sha256:{}", hash('a'));
    for contents in [
        format!("{pin}\nSAMPLE==2.0 --hash=sha256:{}", hash('b')),
        "sample==1.0 --hash=sha256:xyz".into(),
        "sample==1.0 --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --hash=sha1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        "sample==* --hash=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
    ] {
        assert!(package_identities_from_contents(&contents).is_err());
    }
}

#[test]
fn reads_only_a_regular_root_requirements_file() {
    let root = std::env::temp_dir().join(format!("koolade-pip-lock-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("requirements.txt"),
        format!("sample==1.0 --hash=sha256:{}\n", hash('d')),
    )
    .unwrap();
    assert_eq!(collect(&root).unwrap().len(), 1);
    fs::remove_file(root.join("requirements.txt")).unwrap();
    fs::write(root.join("other.txt"), "requirements.txt is absent").unwrap();
    std::os::unix::fs::symlink(root.join("other.txt"), root.join("requirements.txt")).unwrap();
    assert!(collect(&root).is_err());
    fs::remove_dir_all(root).unwrap();
}
