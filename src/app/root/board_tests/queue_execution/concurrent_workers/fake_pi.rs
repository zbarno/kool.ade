use std::path::{Path, PathBuf};

pub(super) fn create(root: &Path) -> PathBuf {
    let pi = root.join("pi-fixture");
    std::fs::write(
        &pi,
        r#"#!/usr/bin/python3
import json, pathlib, sys, time
if '--help' in sys.argv:
    print('''--print
--mode <mode> text, json, rpc
--no-session
--no-approve
--append-system-prompt
--thinking <level> xhigh
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
if prompt.startswith('PROJECT MANAGER UPDATE'):
    assert '--no-tools' in sys.argv
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':'Manager fixture: I am monitoring the assigned worker.'}]}]}))
    sys.exit(0)
if 'TICKET PATH: ' not in prompt:
    root = pathlib.Path(__file__).parent
    deadline = time.time() + 45
    while not (root / '004-task.started').exists() and time.time() < deadline:
        time.sleep(0.05)
    time.sleep(0.25)
    report = {'schemaVersion':1,'assistantMessage':'Planning fixture: I can discuss this while the worker runs.','openItemsAdded':[],'openItemsUpdated':[],'openItemsResolved':[]}
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':json.dumps(report)}]}]}))
    sys.exit(0)
time.sleep(0.3)
ticket = prompt.split('TICKET PATH: ', 1)[1].splitlines()[0]
name = pathlib.Path(ticket).stem + '.txt'
root = pathlib.Path(__file__).parent
if name in ('001-task.txt', '002-task.txt'):
    (root / (name + '.started')).touch()
elif name == '004-task.txt':
    (root / '004-task.started').touch()
elif name == '003-task.txt':
    assert pathlib.Path('001-task.txt').exists() and pathlib.Path('002-task.txt').exists(), 'Dependencies were not merged before starting'
pathlib.Path(name).write_text('implemented')
report = {'status':'complete','summary':'Implemented fixture task','acceptance_criteria':[{'criterion':'File exists.','evidence':'File exists and was verified'}],'verification':['test -f '+name],'remaining':[]}
print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':json.dumps(report)}]}]}))
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o700)).unwrap();
    pi
}
