//! Core delegation pipeline — copy files, spawn agent, monitor, sync back.
//!
//! TODO: re-enable convergio-file-transport when the crate is published on GitHub.
//! Currently, file-transport calls are stubbed out because the dependency is unavailable.

use crate::queries::{complete_delegation, update_delegation_status, update_remote_path};
use crate::types::{validate_shell_path, DelegationStatus, DelegationStep, PipelineConfig};
use convergio_db::pool::ConnPool;
use convergio_mesh::peers_registry::peers_conf_path_from_env;
use convergio_mesh::peers_types::PeersRegistry;
use std::path::Path;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

/// Info returned by a successful pipeline run, used to start the monitor.
pub struct PipelineResult {
    pub ssh_target: String,
    pub tmux_session: String,
    pub tmux_window: String,
    pub remote_path: String,
}

/// Resolve the SSH target string for a peer from the peers registry.
fn resolve_ssh_target(peer_name: &str) -> Result<String, BoxErr> {
    let conf_path = peers_conf_path_from_env();
    let registry = PeersRegistry::load(Path::new(&conf_path))?;
    let peer_cfg = registry
        .peers
        .get(peer_name)
        .ok_or_else(|| format!("peer '{peer_name}' not found in peers.conf"))?;
    if peer_cfg.ssh_alias.is_empty() {
        Ok(format!("{}@{}", peer_cfg.user, peer_cfg.tailscale_ip))
    } else {
        Ok(peer_cfg.ssh_alias.clone())
    }
}

/// Run the full delegation pipeline: sync -> copy -> spawn -> set Running.
/// Returns info needed to start the remote monitor.
pub async fn run_delegation_pipeline(
    pool: &ConnPool,
    delegation_id: &str,
    plan_id: i64,
    peer_name: &str,
    config: &PipelineConfig,
) -> Result<PipelineResult, BoxErr> {
    let ssh_target = resolve_ssh_target(peer_name)?;
    validate_shell_path(&ssh_target)?;
    // Sync directly to peer's repo (not a temp subdirectory)
    let remote_path = config.remote_base.clone();
    validate_shell_path(&remote_path)?;
    validate_shell_path(&config.project_root)?;

    // Step 1: rsync files to peer's repo (including .git for push/PR)
    update_delegation_status(
        pool,
        delegation_id,
        &DelegationStatus::CopyingFiles,
        &DelegationStep::FileCopy,
    )?;

    // TODO: re-enable when convergio-file-transport is available
    // let push_req = convergio_file_transport::types::TransferRequest {
    //     source_path: config.project_root.clone(),
    //     dest_path: remote_path.clone(),
    //     peer_name: peer_name.to_string(),
    //     ssh_target: ssh_target.clone(),
    //     direction: convergio_file_transport::types::TransferDirection::Push,
    //     exclude_patterns: config.exclude_patterns.clone(),
    // };
    // let push_result =
    //     convergio_file_transport::rsync::execute_rsync(&push_req, &ssh_target).await?;
    // if let convergio_file_transport::types::TransferStatus::Failed(msg) = &push_result.status {
    //     let err = DelegationStatus::Failed(format!("file copy failed: {msg}"));
    //     update_delegation_status(pool, delegation_id, &err, &DelegationStep::FileCopy)?;
    //     return Err(format!("file copy failed: {msg}").into());
    // }
    // if let Ok(conn) = pool.get() {
    //     let _ = convergio_file_transport::transfer::record_transfer(&conn, &push_result, &push_req);
    // }

    // Stub: run rsync via plain SSH until file-transport is available
    let excludes: String = config
        .exclude_patterns
        .iter()
        .map(|p| format!(" --exclude={p}"))
        .collect();
    let rsync_cmd = format!(
        "rsync -az --delete{excludes} {} {}:{}/",
        config.project_root, ssh_target, remote_path
    );
    let rsync_out = tokio::process::Command::new("sh")
        .args(["-c", &rsync_cmd])
        .output()
        .await;
    match &rsync_out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let err = DelegationStatus::Failed(format!("file copy failed: {stderr}"));
            update_delegation_status(pool, delegation_id, &err, &DelegationStep::FileCopy)?;
            return Err(format!("file copy failed: {stderr}").into());
        }
        Err(e) => {
            let err = DelegationStatus::Failed(format!("file copy failed: {e}"));
            update_delegation_status(pool, delegation_id, &err, &DelegationStep::FileCopy)?;
            return Err(format!("file copy failed: {e}").into());
        }
    }

    // Step 1b: Build daemon + CLI on peer via SSH
    let daemon_dir = format!("{remote_path}/daemon");
    let build_cmd = format!(
        "cd {daemon_dir} && cargo build --release && \
         cargo install --path crates/convergio-cli --force"
    );
    tracing::info!(delegation_id, peer_name, "building daemon + CLI on peer");
    let build_out = std::process::Command::new("ssh")
        .args([&ssh_target, &build_cmd])
        .output();
    match &build_out {
        Ok(o) if o.status.success() => {
            tracing::info!(delegation_id, "remote build + CLI install OK");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tracing::warn!(delegation_id, %stderr, "remote build failed");
        }
        Err(e) => {
            tracing::warn!(delegation_id, %e, "ssh build command failed");
        }
    }

    // Step 1c: Restart daemon on peer so new binary takes effect
    let restart_cmd = "if command -v launchctl >/dev/null 2>&1; then \
         launchctl unload ~/Library/LaunchAgents/com.convergio.daemon.plist 2>/dev/null; \
         launchctl load ~/Library/LaunchAgents/com.convergio.daemon.plist; \
     else \
         systemctl --user restart convergio 2>/dev/null || \
         (pkill -f convergio-daemon; nohup ~/.local/bin/convergio &); \
     fi"
    .to_string();
    tracing::info!(delegation_id, peer_name, "restarting daemon on peer");
    let restart_out = std::process::Command::new("ssh")
        .args([&ssh_target, &restart_cmd])
        .output();
    match &restart_out {
        Ok(o) if o.status.success() => {
            tracing::info!(delegation_id, "daemon restart OK on peer");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            tracing::warn!(delegation_id, %stderr, "daemon restart warning");
        }
        Err(e) => {
            tracing::warn!(delegation_id, %e, "ssh restart command failed");
        }
    }
    // Give daemon time to start before spawning agent
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Step 2: Spawn agent on peer
    update_delegation_status(
        pool,
        delegation_id,
        &DelegationStatus::Spawning,
        &DelegationStep::Spawn,
    )?;
    let tmux_session = format!("cvg-{delegation_id}");
    let tmux_window = format!("plan-{plan_id}");
    crate::remote_spawn::spawn_on_peer(
        &ssh_target,
        &remote_path,
        plan_id,
        &tmux_session,
        &tmux_window,
    )
    .await?;

    // Step 3: Running — monitoring happens asynchronously
    update_delegation_status(
        pool,
        delegation_id,
        &DelegationStatus::Running,
        &DelegationStep::Execute,
    )?;
    update_remote_path(pool, delegation_id, &remote_path)?;
    tracing::info!(delegation_id, peer_name, "delegation running on peer");
    Ok(PipelineResult {
        ssh_target,
        tmux_session,
        tmux_window,
        remote_path,
    })
}

/// Sync results back from the remote peer after completion.
pub async fn sync_back(
    pool: &ConnPool,
    delegation_id: &str,
    peer_name: &str,
    config: &PipelineConfig,
) -> Result<(), BoxErr> {
    let ssh_target = resolve_ssh_target(peer_name)?;
    validate_shell_path(&ssh_target)?;
    // Sync directly to peer's repo (not a temp subdirectory)
    let remote_path = config.remote_base.clone();
    validate_shell_path(&remote_path)?;
    validate_shell_path(&config.project_root)?;

    update_delegation_status(
        pool,
        delegation_id,
        &DelegationStatus::SyncingBack,
        &DelegationStep::SyncBack,
    )?;

    // TODO: re-enable when convergio-file-transport is available
    // let pull_req = convergio_file_transport::types::TransferRequest {
    //     source_path: remote_path,
    //     dest_path: config.project_root.clone(),
    //     peer_name: peer_name.to_string(),
    //     ssh_target: ssh_target.clone(),
    //     direction: convergio_file_transport::types::TransferDirection::Pull,
    //     exclude_patterns: config.exclude_patterns.clone(),
    // };
    // convergio_file_transport::rsync::execute_rsync(&pull_req, &ssh_target).await?;

    // Stub: run rsync via plain SSH until file-transport is available
    let excludes: String = config
        .exclude_patterns
        .iter()
        .map(|p| format!(" --exclude={p}"))
        .collect();
    let rsync_cmd = format!(
        "rsync -az --delete{excludes} {}:{}/ {}/",
        ssh_target, remote_path, config.project_root
    );
    let rsync_out = tokio::process::Command::new("sh")
        .args(["-c", &rsync_cmd])
        .output()
        .await;
    match rsync_out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            return Err(format!("sync_back rsync failed: {stderr}").into());
        }
        Err(e) => return Err(format!("sync_back rsync failed: {e}").into()),
    }

    update_delegation_status(
        pool,
        delegation_id,
        &DelegationStatus::Done,
        &DelegationStep::Complete,
    )?;
    complete_delegation(pool, delegation_id)?;
    Ok(())
}
