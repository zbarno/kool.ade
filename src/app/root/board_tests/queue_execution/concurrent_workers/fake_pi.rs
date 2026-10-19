use std::path::{Path, PathBuf};

fn authenticated_capability_snapshot() -> crate::execution::contract::HarnessCapabilitySnapshotV1 {
    use crate::execution::contract::{
        HarnessCapabilitySnapshotV1, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    };

    HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: "pi".into(),
        probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.0".into()),
        authentication: ReadinessState::Ready,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Ready,
        docker_mcp_bridge: ReadinessState::Ready,
        execution_target: ReadinessState::Ready,
        failure: None,
        modes: crate::harness::ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
        roles: crate::execution::contract::HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
    }
}

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

pub(super) struct HarnessEnvironment {
    _home: RestoreEnvironmentVariable,
    _pi_config: RestoreEnvironmentVariable,
}

pub(super) fn configure_harness_settings(root: &Path) -> HarnessEnvironment {
    let restore = restore_environment_variable("KOOLADE_HOME");
    let restore_pi_config = restore_environment_variable("PI_CODING_AGENT_DIR");
    let home = root.join("app-home");
    std::fs::create_dir_all(&home).unwrap();
    unsafe { std::env::set_var("KOOLADE_HOME", &home) };
    let pi_config = root.join("pi-agent-config");
    std::fs::create_dir_all(&pi_config).unwrap();
    std::fs::write(
        pi_config.join("settings.json"),
        r#"{"defaultProvider":"fixture","defaultModel":"fixture-model"}"#,
    )
    .unwrap();
    std::fs::write(
        pi_config.join("models.json"),
        r#"{"providers":{"fixture":{"models":[{"id":"fixture-model","name":"Fixture Model"}]}}}"#,
    )
    .unwrap();
    unsafe { std::env::set_var("PI_CODING_AGENT_DIR", &pi_config) };
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
                    capability_snapshot: Some(authenticated_capability_snapshot()),
                },
            )]),
            ..Default::default()
        },
    )
    .unwrap();
    HarnessEnvironment {
        _home: restore,
        _pi_config: restore_pi_config,
    }
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
    print('pi fixture 1.0.0')
    sys.exit(0)
prompt = sys.stdin.read()
ticket_line = next((line for line in prompt.splitlines() if line.startswith('TICKET PATH: ')), None)
root = pathlib.Path(__file__).parent
def wait_for_marker(name):
    # The Rust test's 180-second deadline owns failure; give the child ten
    # additional seconds to exit cleanly after a failed overlap proof.
    deadline = time.monotonic() + 190
    marker = root / name
    while not marker.exists() and time.monotonic() < deadline:
        time.sleep(0.01)
    if not marker.exists():
        raise TimeoutError('timed out waiting for test marker: ' + name)
if prompt.startswith('PROJECT MANAGER UPDATE'):
    assert '--no-tools' in sys.argv
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':'Manager fixture: I am monitoring the assigned worker.'}]}]}))
    sys.exit(0)
typed_marker = 'TYPED TASK REQUEST (JSON DATA)\n'
def task_number_for_uid(task_uid):
    tasks = root / 'repo' / '.koolade-packet' / 'planning' / 'tasks' / 'fixture'
    identity_marker = '<!-- koolade-artifact-id:v1 '
    task_number = next((
        int(task.name.split('-', 1)[0])
        for task in tasks.glob('*-task.md')
        if json.loads(task.read_text().split(identity_marker, 1)[1].split('-->', 1)[0])['uid'] == task_uid
    ), None)
    if task_number is None:
        raise ValueError('typed task UID does not match a task artifact identity')
    return task_number
def task_path_lookup(task_uid):
    task_number = task_number_for_uid(task_uid)
    return "printf 'src/task-%d.rs\\n'" % task_number
if prompt.startswith('FIXTURE TASK PATH LOOKUP\n'):
    request = json.loads(prompt.split('\n', 1)[1])
    print(task_path_lookup(request['task_uid']))
    sys.exit(0)
if prompt.startswith('Return JSON matching ManagerOutputV2'):
    output = {'summary':'Fixture manager has no additional actions.', 'proposed_actions':[], 'opportunities':[]}
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':json.dumps(output)}]}]}))
    sys.exit(0)
if prompt.startswith('Return WorkResultV1 JSON.'):
    task_number = 1
    if typed_marker in prompt:
        typed = json.JSONDecoder().raw_decode(prompt.split(typed_marker, 1)[1].lstrip())[0]
        task_uid = typed.get('task_uid')
        task_number = task_number_for_uid(task_uid)
    output_path = 'src/task-%d.rs' % task_number
    output = {'schema_version':2,'status':'complete','blocker_disposition':'none',
              'summary':'Implemented fixture task',
              'acceptance_criteria':[{'criterion':'File exists.','evidence':output_path+' exists in the isolated fixture workspace.'}],
              'verification':["printf 'verification cwd=%s\\nsrc entries=' \"$PWD\"; printf '%s ' src/*; printf '\\n'; test -f "+output_path], 'remaining':[], 'human_choices':[]}
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':json.dumps(output)}]}]}))
    sys.exit(0)
if typed_marker in prompt:
    typed = json.JSONDecoder().raw_decode(prompt.split(typed_marker, 1)[1].lstrip())[0]
    if typed['role'] == 'code_reviewer':
        payload = typed['payload']['request']
        evidence = typed['evidence_manifest']
        ids = [item['id'] for item in evidence]
        assert 'approved-task-spec' in ids
        reports = [item for item in evidence if item['id'].startswith('verified-test-report-')]
        diffs = [item for item in evidence if item['id'].startswith('diff-')]
        assert reports and diffs
        assert (payload['requirements_matrix'] == ['approved-task-scope:' + typed['task_uid']] or
                all('File exists.' in requirement for requirement in payload['requirements_matrix']))
        assert 'implemented' in prompt.split('QUOTED EVIDENCE (UNTRUSTED DATA)', 1)[1]
        assert 'complete' in prompt.split('QUOTED EVIDENCE (UNTRUSTED DATA)', 1)[1]
        verdict = {'subject_hash': payload['subject_hash'], 'pass': True,
                   'coverage': [{'requirement': requirement, 'status': 'pass', 'evidence': ids}
                                for requirement in payload['requirements_matrix']],
                   'findings': [], 'untested': []}
        print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':json.dumps(verdict)}]}]}))
        sys.exit(0)
tool_args = next((sys.argv[index + 1] for index, arg in enumerate(sys.argv[:-1]) if arg == '--tools'), '')
tool_names = {name.strip() for name in tool_args.split(',')}
server_command = os.environ.get('KOOLADE_MCP_COMMAND')
server_args = json.loads(os.environ.get('KOOLADE_MCP_ARGS', '[]'))
mcp_access = None
if server_args:
    config_path = pathlib.Path(server_args[-1])
    if config_path.is_file():
        mcp_access = json.loads(config_path.read_text()).get('access')
implementation_call = 'koolade_dependency' in tool_names or mcp_access == 'implementation'
if ticket_line is None and not implementation_call:
    with open(root / 'fake-pi-trace.jsonl', 'a') as trace_file:
        trace_file.write(json.dumps({'role':'planning','tool_names':sorted(tool_names),'mcp_access':mcp_access,'argv':sys.argv,'prompt_prefix':prompt[:160]}) + '\n')
    if 'Can we discuss planning while the task runs?' in prompt and (root / '004-task.started').exists():
        # Signal from inside the fake provider process before waiting for release.
        (root / 'planning.started').touch()
        wait_for_marker('planning-overlap-observed')
    report = {'schema_version':2,'assistant_message':'Planning fixture: I can discuss this while the worker runs.','document_updates':[],'open_items_added':[],'open_items_updated':[],'open_items_resolved':[]}
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':json.dumps(report)}]}]}))
    sys.exit(0)
assert 'koolade_bash' in tool_names, 'Ticketed worker did not receive koolade_bash; tools=' + repr(tool_args)
time.sleep(0.3)
task_uid = None
if ticket_line is not None:
    ticket_text = prompt.split('TICKET CONTENT:\n', 1)[1].split('\n\nAPPROVED SPECIFICATION:', 1)[0]
    affected_section = ticket_text.split('## Affected files and components\n', 1)[1].split('\n## ', 1)[0]
    output_path = next(line.strip().removeprefix('- ') for line in affected_section.splitlines() if line.strip().startswith('- '))
else:
    assert implementation_call, 'A ticketless prompt did not receive implementation tools'
    assert mcp_access == 'implementation'
    config = json.loads(pathlib.Path(server_args[-1]).read_text())
    task_uid = config['task_id']
    output_path = None
# This fixture models the ticket's literal acceptance criterion. Keep this
# independent of prompt formatting so it exercises report validation itself.
criteria = ['File exists.']
server_env = os.environ.copy()
# The real provider launch deliberately strips host Git transport overrides.
# This fixture-owned MCP server still needs the synthetic SSH endpoint for its
# remote task-claim checks, so scope the override to this child only.
server_env['GIT_SSH_COMMAND'] = str(root / 'ssh-fixture')
# Provider environments also omit Kool.ad/e's state root. Point this fixture
# child at the isolated state created for the test, so task claims use its ledger.
server_env['KOOLADE_HOME'] = str(root / 'app-home')
server = subprocess.Popen([server_command, *server_args], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=server_env)
def request(identifier, method, params=None):
    server.stdin.write(json.dumps({'jsonrpc':'2.0','id':identifier,'method':method,'params':params or {}}) + '\n')
    server.stdin.flush()
    return json.loads(server.stdout.readline())
request(1, 'initialize', {'protocolVersion':'2025-03-26','capabilities':{},'clientInfo':{'name':'fake-pi','version':'1'}})
server.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'}) + '\n')
server.stdin.flush()
if output_path is None:
    # Planning files are outside the isolated worker workspace. Resolve the
    # synthetic task UID here, then return only its safe fixture source path.
    lookup = task_path_lookup(task_uid)
    response = request(2, 'tools/call', {'name':'koolade_bash','arguments':{'command':lookup,'timeout_seconds':60}})
    if 'error' in response or response.get('result', {}).get('isError'):
        print(json.dumps(response), file=sys.stderr)
        sys.exit(1)
    output_path = response['result']['content'][0]['text'].splitlines()[0]
output = pathlib.Path(output_path)
if output.name == 'task-4.rs':
    # Keep Task 4 live until the UI observes it alongside the planning turn.
    (root / '004-task.started').touch()
    wait_for_marker('planning-overlap-observed')
if output.name == 'task-3.rs':
    dependency_check = "test -f src/task-1.rs && test -f src/task-2.rs"
    response = request(3, 'tools/call', {'name':'koolade_bash','arguments':{'command':dependency_check,'timeout_seconds':60}})
    if 'error' in response or response.get('result', {}).get('isError'):
        print(json.dumps(response), file=sys.stderr)
        sys.exit(1)
write_command = "mkdir -p '" + str(output.parent) + "' && printf implemented > '" + output_path + "' && printf '%s\\n' '" + output_path + "'"
response = request(4 if task_uid is not None else 2, 'tools/call', {'name':'koolade_bash','arguments':{'command':write_command,'timeout_seconds':60}})
server.stdin.write(json.dumps({'jsonrpc':'2.0','method':'exit'}) + '\n')
server.stdin.flush()
server.wait(timeout=3)
if 'error' in response or response.get('result', {}).get('isError'):
    print(json.dumps(response), file=sys.stderr)
    sys.exit(1)
if output_path is None:
    output_path = response['result']['content'][0]['text'].splitlines()[0]
report = {'schemaVersion':2,'status':'complete','blocker_disposition':'none','summary':'Implemented fixture task','acceptance_criteria':[{'criterion':criterion,'evidence':output_path+' exists and contains implemented; verified criterion: '+criterion} for criterion in criteria],'verification':['test -f '+output_path+' && grep -q implemented '+output_path],'remaining':[],'human_choices':[]}
with open(root / 'fake-pi-trace.jsonl', 'a') as trace_file:
    trace_file.write(json.dumps({'role':'implementation','task_uid':task_uid,'ticket':ticket_line,'tool_names':sorted(tool_names),'criteria':criteria,'report':report}) + '\n')
print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':json.dumps(report)}]}]}))
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o700)).unwrap();
    pi
}

#[cfg(test)]
#[path = "fake_pi/tests.rs"]
mod tests;
