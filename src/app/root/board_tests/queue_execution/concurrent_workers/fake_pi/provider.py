#!/usr/bin/python3
import atexit, functools, json, os, pathlib, subprocess, sys, threading, time
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
def drain_mcp_stdout(stream):
    try:
        while stream.read(65536):
            pass
    except (OSError, ValueError):
        pass
def reap_mcp_server(server):
    drainer = None
    if server.stdout and not server.stdout.closed:
        drainer = threading.Thread(target=drain_mcp_stdout, args=(server.stdout,), daemon=True)
        drainer.start()
    try:
        if server.poll() is None:
            try:
                if server.stdin and not server.stdin.closed:
                    server.stdin.write(json.dumps({'jsonrpc':'2.0','method':'exit'}) + '\n')
                    server.stdin.flush()
                    server.stdin.close()
            except OSError:
                pass
            try:
                server.wait(timeout=30)
            except subprocess.TimeoutExpired:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()
                raise TimeoutError('fixture MCP server did not stop after exit notification')
    finally:
        if drainer is not None:
            drainer.join(timeout=5)
    if server.returncode != 0:
        raise RuntimeError('fixture MCP server exited with status ' + str(server.returncode))
def cleanup_mcp_server(server):
    try:
        reap_mcp_server(server)
    except Exception as error:
        if server.poll() is None:
            server.kill()
            server.wait()
        print('fixture MCP server cleanup failed: ' + str(error), file=sys.stderr)
mcp_cleanup_callbacks = {}
def spawn_mcp_server(command, args, env):
    server = subprocess.Popen([command, *args], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, text=True, env=env)
    callback = functools.partial(cleanup_mcp_server, server)
    mcp_cleanup_callbacks[id(server)] = callback
    atexit.register(callback)
    return server
def stop_mcp_server(server):
    try:
        reap_mcp_server(server)
    finally:
        callback = mcp_cleanup_callbacks.pop(id(server), None)
        if callback is not None:
            atexit.unregister(callback)
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
    write_output = ''
    if typed_marker in prompt:
        typed = json.JSONDecoder().raw_decode(prompt.split(typed_marker, 1)[1].lstrip())[0]
        task_uid = typed.get('task_uid')
        task_number = task_number_for_uid(task_uid)
    output_path = 'src/task-%d.rs' % task_number
    early_tool_args = next((sys.argv[index + 1] for index, arg in enumerate(sys.argv[:-1]) if arg == '--tools'), '')
    early_tool_names = {name.strip() for name in early_tool_args.split(',')}
    early_server_command = os.environ.get('KOOLADE_MCP_COMMAND')
    early_server_args = json.loads(os.environ.get('KOOLADE_MCP_ARGS', '[]'))
    early_config_path = pathlib.Path(early_server_args[-1]) if early_server_args else None
    early_config = json.loads(early_config_path.read_text()) if early_config_path and early_config_path.is_file() else {}
    if early_config.get('access') == 'implementation' and 'koolade_bash' in early_tool_names:
        early_env = os.environ.copy()
        early_env['GIT_SSH_COMMAND'] = str(root / 'ssh-fixture')
        early_env['KOOLADE_HOME'] = str(root / 'app-home')
        early_server = spawn_mcp_server(early_server_command, early_server_args, early_env)
        def early_request(identifier, method, params=None):
            early_server.stdin.write(json.dumps({'jsonrpc':'2.0','id':identifier,'method':method,'params':params or {}}) + '\n')
            early_server.stdin.flush()
            return json.loads(early_server.stdout.readline())
        early_request(1, 'initialize', {'protocolVersion':'2025-03-26','capabilities':{},'clientInfo':{'name':'fake-pi','version':'1'}})
        early_server.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'}) + '\n')
        early_server.stdin.flush()
        write_command = "printf 'write cwd=%s\\n' \"$PWD\"; mkdir -p src && printf implemented > '" + output_path + "' && printf 'wrote=%s\\n' \"$PWD/" + output_path + "\" && test -f '" + output_path + "'"
        early_response = early_request(2, 'tools/call', {'name':'koolade_bash','arguments':{'command':write_command,'timeout_seconds':60}})
        stop_mcp_server(early_server)
        if 'error' in early_response or early_response.get('result', {}).get('isError'):
            print(json.dumps(early_response), file=sys.stderr)
            sys.exit(1)
        write_output = early_response.get('result', {}).get('content', [{}])[0].get('text', '')
        if output_path == 'src/task-4.rs':
            # Keep Task 4 active until the UI observes it alongside planning.
            (root / '004-task.started').touch()
            wait_for_marker('planning-overlap-observed')
    if output_path in ('src/task-1.rs', 'src/task-2.rs') and (root / 'capacity-wait-barrier-enabled').exists():
        # Hold both worker slots until the UI observes Task 4 waiting for capacity.
        wait_for_marker('capacity-wait-observed')
    output = {'schema_version':2,'status':'complete','blocker_disposition':'none',
              'summary':'Implemented fixture task',
              'acceptance_criteria':[{'criterion':'File exists.','evidence':output_path+' exists in the isolated fixture workspace.'}],
              'verification':["printf 'verification cwd=%s\\nsrc entries=' \"$PWD\"; printf '%s ' src/*; printf '\\n'; test -f "+output_path], 'remaining':[], 'human_choices':[]}
    if write_output:
        with open(root / 'fake-pi-trace.jsonl', 'a') as trace_file:
            trace_file.write(json.dumps({'role':'implementation','write_output':write_output,'report':output}) + '\n')
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
        quoted_evidence = prompt.split('QUOTED EVIDENCE (UNTRUSTED DATA)', 1)[1]
        quoted_items = [json.loads(line) for line in quoted_evidence.splitlines() if line.strip()]
        subject_item = next((item for item in quoted_items
                             if item.get('id') == 'review-subject-v1'), None)
        assert subject_item and subject_item.get('content')
        subject = json.loads(subject_item['content'])
        assert reports and diffs
        assert (payload['requirements_matrix'] == ['approved-task-scope:' + typed['task_uid']] or
                all('File exists.' in requirement for requirement in payload['requirements_matrix']))
        assert 'implemented' in prompt.split('QUOTED EVIDENCE (UNTRUSTED DATA)', 1)[1]
        assert 'complete' in prompt.split('QUOTED EVIDENCE (UNTRUSTED DATA)', 1)[1]
        verdict = {'subject_hash': payload['subject_hash'], 'pass': True,
                   'outcome': 'pass', 'examined_files': subject['affected_paths'],
                   'tested_evidence_ids': [item['id'] for item in reports],
                   'unexamined_areas': [],
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
        trace_file.write(json.dumps({'role':'planning','tool_names':sorted(tool_names),'mcp_access':mcp_access,'provider':os.environ.get('PI_PROVIDER'),'model':os.environ.get('PI_MODEL'),'no_tools':'--no-tools' in sys.argv}) + '\n')
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
server = spawn_mcp_server(server_command, server_args, server_env)
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
if output.name in ('task-1.rs', 'task-2.rs') and (root / 'capacity-wait-barrier-enabled').exists():
    # Hold both worker slots until the UI observes Task 4 waiting for capacity.
    wait_for_marker('capacity-wait-observed')
if output.name == 'task-3.rs':
    dependency_check = "test -f src/task-1.rs && test -f src/task-2.rs"
    response = request(3, 'tools/call', {'name':'koolade_bash','arguments':{'command':dependency_check,'timeout_seconds':60}})
    if 'error' in response or response.get('result', {}).get('isError'):
        print(json.dumps(response), file=sys.stderr)
        sys.exit(1)
write_command = "printf 'write cwd=%s\\n' \"$PWD\"; mkdir -p '" + str(output.parent) + "' && printf implemented > '" + output_path + "' && printf 'wrote=%s\\n' \"$PWD/" + output_path + "\" && test -f '" + output_path + "'"
response = request(4 if task_uid is not None else 2, 'tools/call', {'name':'koolade_bash','arguments':{'command':write_command,'timeout_seconds':60}})
stop_mcp_server(server)
if 'error' in response or response.get('result', {}).get('isError'):
    print(json.dumps(response), file=sys.stderr)
    sys.exit(1)
write_output = response.get('result', {}).get('content', [{}])[0].get('text', '')
if output_path is None:
    output_path = response['result']['content'][0]['text'].splitlines()[0]
report = {'schemaVersion':2,'status':'complete','blocker_disposition':'none','summary':'Implemented fixture task','acceptance_criteria':[{'criterion':criterion,'evidence':output_path+' exists and contains implemented; verified criterion: '+criterion} for criterion in criteria],'verification':['test -f '+output_path+' && grep -q implemented '+output_path],'remaining':[],'human_choices':[]}
with open(root / 'fake-pi-trace.jsonl', 'a') as trace_file:
    trace_file.write(json.dumps({'role':'implementation','task_uid':task_uid,'ticket':ticket_line,'tool_names':sorted(tool_names),'criteria':criteria,'write_output':write_output,'report':report}) + '\n')
print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':json.dumps(report)}]}]}))
