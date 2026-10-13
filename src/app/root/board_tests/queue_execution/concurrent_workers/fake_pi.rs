use std::path::{Path, PathBuf};

pub(super) fn sandbox_available() -> bool {
    crate::harness::runtime_capabilities::RuntimeCapabilities::detect().implementation
}

pub(super) struct RestoreEnvironmentVariable {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl Drop for RestoreEnvironmentVariable {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

pub(super) fn restore_environment_variable(name: &'static str) -> RestoreEnvironmentVariable {
    RestoreEnvironmentVariable {
        name,
        previous: std::env::var_os(name),
    }
}

pub(super) fn configure_harness_settings(root: &Path) -> RestoreEnvironmentVariable {
    let restore = restore_environment_variable("KOOLADE_HOME");
    let home = root.join("app-home");
    std::fs::create_dir_all(&home).unwrap();
    unsafe { std::env::set_var("KOOLADE_HOME", &home) };
    crate::persistence::harness_settings::save(
        &crate::persistence::harness_settings::HarnessSettings {
            discovered: std::collections::BTreeMap::from([(
                "pi".into(),
                crate::persistence::harness_settings::DetectedHarness {
                    status: "pi fixture 1.0".into(),
                    version: Some("1.0".into()),
                    executable: None,
                    diagnostic: None,
                    ready: true,
                    models: Vec::new(),
                    default_model: None,
                    model_catalog_fresh: false,
                    configuration_required: false,
                    implementation_available: true,
                    checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                    capability_snapshot: None,
                },
            )]),
            ..Default::default()
        },
    )
    .unwrap();
    restore
}

pub(super) fn create(root: &Path) -> PathBuf {
    let pi = root.join("pi-fixture");
    std::fs::write(
        &pi,
        r#"#!/usr/bin/python3
import json, os, pathlib, subprocess, sys, time
if '--help' in sys.argv:
    print('''--print
--mode <mode> text, json, rpc
--no-session
--no-approve
--append-system-prompt
--provider <name>
--model <pattern>
--thinking <level> xhigh
--no-themes
--no-extensions
--no-skills
--no-prompt-templates
--no-context-files
--no-tools
--tools
--no-builtin-tools
--extension''')
    sys.exit(0)
if '--version' in sys.argv:
    print('pi fixture 1.0')
    sys.exit(0)
prompt = sys.stdin.read()
ticket_line = next((line for line in prompt.splitlines() if line.startswith('TICKET PATH: ')), None)
if prompt.startswith('PROJECT MANAGER UPDATE'):
    assert '--no-tools' in sys.argv
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':'Manager fixture: I am monitoring the assigned worker.'}]}]}))
    sys.exit(0)
tool_args = next((sys.argv[index + 1] for index, arg in enumerate(sys.argv[:-1]) if arg == '--tools'), '')
if ticket_line is None or 'koolade_bash' not in tool_args.split(','):
    root = pathlib.Path(__file__).parent
    deadline = time.time() + 45
    while not (root / '004-task.started').exists() and time.time() < deadline:
        time.sleep(0.05)
    time.sleep(0.25)
    report = {'schemaVersion':1,'assistantMessage':'Planning fixture: I can discuss this while the worker runs.','openItemsAdded':[],'openItemsUpdated':[],'openItemsResolved':[]}
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':json.dumps(report)}]}]}))
    sys.exit(0)
time.sleep(0.3)
ticket = ticket_line.removeprefix('TICKET PATH: ')
name = pathlib.Path(ticket).stem + '.txt'
root = pathlib.Path(__file__).parent
if name in ('001-task.txt', '002-task.txt'):
    (root / (name + '.started')).touch()
elif name == '004-task.txt':
    (root / '004-task.started').touch()
elif name == '003-task.txt':
    assert pathlib.Path('001-task.txt').exists() and pathlib.Path('002-task.txt').exists(), 'Dependencies were not merged before starting'
server_command = os.environ['KOOLADE_MCP_COMMAND']
server_args = json.loads(os.environ['KOOLADE_MCP_ARGS'])
server = subprocess.Popen([server_command, *server_args], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
def request(identifier, method, params=None):
    server.stdin.write(json.dumps({'jsonrpc':'2.0','id':identifier,'method':method,'params':params or {}}) + '\n')
    server.stdin.flush()
    return json.loads(server.stdout.readline())
request(1, 'initialize', {'protocolVersion':'2025-03-26','capabilities':{},'clientInfo':{'name':'fake-pi','version':'1'}})
server.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'}) + '\n')
server.stdin.flush()
response = request(2, 'tools/call', {'name':'koolade_bash','arguments':{'command':"printf implemented > '" + name + "'",'timeout_seconds':60}})
server.stdin.write(json.dumps({'jsonrpc':'2.0','method':'exit'}) + '\n')
server.stdin.flush()
server.wait(timeout=3)
if 'error' in response or response.get('result', {}).get('isError'):
    print(json.dumps(response), file=sys.stderr)
    sys.exit(1)
report = {'schemaVersion':2,'status':'complete','blocker_disposition':'none','summary':'Implemented fixture task','acceptance_criteria':[{'criterion':'File exists.','evidence':'File exists and was verified'}],'verification':['test -f '+name],'remaining':[],'human_choices':[]}
print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':json.dumps(report)}]}]}))
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o700)).unwrap();
    pi
}
