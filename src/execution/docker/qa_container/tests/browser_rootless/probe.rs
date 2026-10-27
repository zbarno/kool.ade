use bollard::{
    container::LogOutput,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};
use futures_util::TryStreamExt;

/// Read a bounded task-service marker through its app-owned preview relay.
pub(super) async fn marker_is_active(
    docker: &bollard::Docker,
    relay_container_id: &str,
    relay_runtime_user: &str,
    path: &str,
    expected_marker: &str,
) -> Result<bool, String> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('?')
        || path.contains('#')
        || path.contains('\\')
    {
        return Err("preview marker path is invalid".into());
    }
    let url = serde_json::to_string(&format!("http://qa-task:4173{path}"))
        .map_err(|_| "could not encode preview marker path".to_string())?;
    let marker = serde_json::to_string(expected_marker)
        .map_err(|_| "could not encode expected preview marker".to_string())?;
    let script = format!(
        "fetch({url},{{signal:AbortSignal.timeout(1000)}}).then(async response=>{{if(!response.ok){{process.exitCode=3;return}}if(!response.body){{process.exitCode=4;return}}const reader=response.body.getReader();const decoder=new TextDecoder();let size=0;let body='';for(;;){{const {{done,value}}=await reader.read();if(done)break;size+=value.byteLength;if(size>32){{await reader.cancel();process.exitCode=5;return}}body+=decoder.decode(value,{{stream:true}})}}body+=decoder.decode();process.stdout.write(body==={marker}?'yes':'no')}}).catch(()=>{{process.exitCode=2}})"
    );
    let exec = docker
        .create_exec(
            relay_container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(false),
                cmd: Some(vec!["node".into(), "-e".into(), script]),
                user: Some(relay_runtime_user.into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| "could not start the read-only preview marker probe".to_string())?;
    let StartExecResults::Attached { mut output, .. } = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(128),
            }),
        )
        .await
        .map_err(|_| "could not attach to the read-only preview marker probe".to_string())?
    else {
        return Err("preview marker probe did not attach".into());
    };
    let mut stdout = Vec::new();
    while let Some(output) = output
        .try_next()
        .await
        .map_err(|_| "preview marker probe output could not be read".to_string())?
    {
        if let LogOutput::StdOut { message } | LogOutput::Console { message } = output {
            stdout.extend_from_slice(&message);
            if stdout.len() > 32 {
                return Err("preview marker probe output exceeded its bound".into());
            }
        }
    }
    let status = docker
        .inspect_exec(&exec.id)
        .await
        .map_err(|_| "preview marker probe result could not be inspected".to_string())?;
    if status.exit_code != Some(0) {
        return Err("preview marker probe exited unsuccessfully".into());
    }
    match stdout.as_slice() {
        b"yes" => Ok(true),
        b"no" => Ok(false),
        _ => Err("preview marker probe returned an unexpected result".into()),
    }
}

/// Read a hostile-sink hit counter through the app-owned preview relay and
/// require the tag to be one of the caller-fixed probe constants. Unlike the
/// marker probe this allows the bounded query string the sink counter needs;
/// the counter body is at most ten digits.
pub(super) async fn sink_hit_count(
    docker: &bollard::Docker,
    relay_container_id: &str,
    relay_runtime_user: &str,
    tag: &str,
) -> Result<u32, String> {
    if !tag
        .chars()
        .all(|character| character.is_ascii_lowercase() || character == '-')
        || tag.is_empty()
        || tag.len() > 32
    {
        return Err("hostile sink probe tag is invalid".into());
    }
    let path = format!("/__sink_hits?tag={tag}");
    let url = serde_json::to_string(&format!("http://qa-task:4173{path}"))
        .map_err(|_| "could not encode hostile sink path".to_string())?;
    let script = format!(
        "fetch({url},{{signal:AbortSignal.timeout(1000)}}).then(async response=>{{if(!response.ok){{process.exitCode=3;return}}if(!response.body){{process.exitCode=4;return}}const reader=response.body.getReader();const decoder=new TextDecoder();let size=0;let body='';for(;;){{const {{done,value}}=await reader.read();if(done)break;size+=value.byteLength;if(size>10){{await reader.cancel();process.exitCode=5;return}}body+=decoder.decode(value,{{stream:true}})}}process.stdout.write(decoder.decode().includes('nul')?'':body)}}).catch(()=>{{process.exitCode=2}})"
    );
    let exec = docker
        .create_exec(
            relay_container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(false),
                cmd: Some(vec!["node".into(), "-e".into(), script]),
                user: Some(relay_runtime_user.into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| "could not start the hostile sink probe".to_string())?;
    let StartExecResults::Attached { mut output, .. } = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(128),
            }),
        )
        .await
        .map_err(|_| "could not attach to the hostile sink probe".to_string())?
    else {
        return Err("hostile sink probe did not attach".into());
    };
    let mut stdout = Vec::new();
    while let Some(output) = output
        .try_next()
        .await
        .map_err(|_| "hostile sink probe output could not be read".to_string())?
    {
        if let LogOutput::StdOut { message } | LogOutput::Console { message } = output {
            stdout.extend_from_slice(&message);
            if stdout.len() > 10 {
                return Err("hostile sink probe output exceeded its bound".into());
            }
        }
    }
    let status = docker
        .inspect_exec(&exec.id)
        .await
        .map_err(|_| "hostile sink probe result could not be inspected".to_string())?;
    if status.exit_code != Some(0) {
        return Err("hostile sink probe exited unsuccessfully".into());
    }
    let text = String::from_utf8(stdout).map_err(|_| "hostile sink probe output is not text")?;
    text.trim()
        .parse::<u32>()
        .map_err(|_| "hostile sink probe returned no count".into())
}
