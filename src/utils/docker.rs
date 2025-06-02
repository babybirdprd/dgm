use crate::DgmResult;
use anyhow::{anyhow, Context};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::{ContainerCreateBody, ContainerInspectResponse, HostConfig};
use bollard::query_parameters::{
    BuildImageOptions, CreateContainerOptions, RemoveContainerOptions, RemoveImageOptions,
    StartContainerOptions, StopContainerOptions, UploadToContainerOptions,
    InspectContainerOptions, DownloadFromContainerOptions,
};
use bollard::Docker;
use bytes::Bytes;
use futures::stream::StreamExt;
use http_body_util::{Either, Full};
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;
use tar::Builder;
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Docker client wrapper for container management
pub struct DockerManager {
    client: Docker,
}

impl DockerManager {
    /// Create a new Docker manager instance
    pub fn new() -> DgmResult<Self> {
        let client = Docker::connect_with_local_defaults()
            .context("Failed to connect to Docker daemon")?;
        Ok(Self { client })
    }

    /// Build a Docker image from a Dockerfile
    pub async fn build_image(
        &self,
        dockerfile_path: &Path,
        image_name: &str,
        force_rebuild: bool,
    ) -> DgmResult<String> {
        // Check if image already exists
        if !force_rebuild {
            if let Ok(_) = self.client.inspect_image(image_name).await {
                info!("Docker image '{}' already exists. Skipping build.", image_name);
                return Ok(image_name.to_string());
            }
        }

        info!("Building Docker image '{}'...", image_name);

        let dockerfile_dir = dockerfile_path
            .parent()
            .ok_or_else(|| anyhow!("Invalid dockerfile path"))?;

        // Create build context tar
        let build_context = self.create_build_context(dockerfile_dir).await?;

        let options = BuildImageOptions {
            dockerfile: dockerfile_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Dockerfile")
                .to_string(),
            t: Some(image_name.to_string()),
            rm: true,
            ..Default::default()
        };

        let body = Either::Left(Full::new(Bytes::from(build_context)));
        let mut stream = self.client.build_image(options, None, Some(body));

        while let Some(msg) = stream.next().await {
            match msg {
                Ok(output) => {
                    if let Some(stream) = output.stream {
                        debug!("Build output: {}", stream.trim());
                    }
                    if let Some(error) = output.error {
                        return Err(anyhow!("Docker build failed: {}", error).into());
                    }
                }
                Err(e) => return Err(anyhow!("Docker build error: {}", e).into()),
            }
        }

        info!("Docker image '{}' built successfully", image_name);
        Ok(image_name.to_string())
    }

    /// Create a build context tar archive from a directory
    async fn create_build_context(&self, context_dir: &Path) -> DgmResult<Vec<u8>> {
        let mut tar_data = Vec::new();
        {
            let mut tar = Builder::new(&mut tar_data);

            // Add all files in the context directory
            for entry in walkdir::WalkDir::new(context_dir) {
                let entry = entry.context("Failed to read directory entry")?;
                let path = entry.path();

                if path.is_file() {
                    let relative_path = path
                        .strip_prefix(context_dir)
                        .context("Failed to create relative path")?;

                    tar.append_path_with_name(path, relative_path)
                        .context("Failed to add file to tar")?;
                }
            }

            tar.finish().context("Failed to finalize tar archive")?;
        }

        Ok(tar_data)
    }

    /// Create and start a container from an image
    pub async fn create_container(
        &self,
        image_name: &str,
        container_name: &str,
        working_dir: Option<&str>,
        env_vars: Option<Vec<String>>,
    ) -> DgmResult<String> {
        // Remove existing container with the same name if it exists
        self.remove_existing_container(container_name).await?;

        let config = ContainerCreateBody {
            image: Some(image_name.to_string()),
            working_dir: working_dir.map(|s| s.to_string()),
            env: env_vars,
            host_config: Some(HostConfig {
                auto_remove: Some(false),
                ..Default::default()
            }),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(true),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: Some(container_name.to_string()),
            ..Default::default()
        };

        let container = self
            .client
            .create_container(Some(options), config)
            .await
            .context("Failed to create container")?;

        info!("Container '{}' created successfully", container_name);
        Ok(container.id)
    }

    /// Start a container
    pub async fn start_container(&self, container_id: &str) -> DgmResult<()> {
        self.client
            .start_container(container_id, None::<StartContainerOptions>)
            .await
            .context("Failed to start container")?;

        info!("Container '{}' started successfully", container_id);
        Ok(())
    }

    /// Stop a container
    pub async fn stop_container(&self, container_id: &str, timeout_secs: u64) -> DgmResult<()> {
        let options = StopContainerOptions {
            t: Some(timeout_secs as i32),
            ..Default::default()
        };

        self.client
            .stop_container(container_id, Some(options))
            .await
            .context("Failed to stop container")?;

        info!("Container '{}' stopped successfully", container_id);
        Ok(())
    }

    /// Remove a container
    pub async fn remove_container(&self, container_id: &str, force: bool) -> DgmResult<()> {
        let options = RemoveContainerOptions {
            force,
            ..Default::default()
        };

        self.client
            .remove_container(container_id, Some(options))
            .await
            .context("Failed to remove container")?;

        info!("Container '{}' removed successfully", container_id);
        Ok(())
    }

    /// Remove existing container with the given name if it exists
    async fn remove_existing_container(&self, container_name: &str) -> DgmResult<()> {
        match self.client.inspect_container(container_name, None::<InspectContainerOptions>).await {
            Ok(container_info) => {
                info!("Removing existing container '{}'", container_name);

                let container_id = container_info.id.clone().unwrap_or_default();

                // Stop the container if it's running
                if let Some(state) = container_info.state {
                    if state.running == Some(true) {
                        if let Err(e) = self.stop_container(&container_id, 10).await {
                            warn!("Failed to stop container before removal: {}", e);
                        }
                    }
                }

                self.remove_container(&container_id, true).await?;
            }
            Err(_) => {
                // Container doesn't exist, which is fine
                debug!("No existing container '{}' found", container_name);
            }
        }
        Ok(())
    }

    /// Execute a command in a container with timeout
    pub async fn exec_command(
        &self,
        container_id: &str,
        command: &[&str],
        timeout_secs: Option<u64>,
    ) -> DgmResult<(String, i64)> {
        let exec_options = CreateExecOptions {
            cmd: Some(command.iter().map(|s| s.to_string()).collect()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let exec = self
            .client
            .create_exec(container_id, exec_options)
            .await
            .context("Failed to create exec")?;

        let exec_start = self.client.start_exec(&exec.id, None).await;

        let (output, exit_code) = match exec_start {
            Ok(StartExecResults::Attached { mut output, .. }) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();

                let collect_output = async {
                    while let Some(Ok(msg)) = output.next().await {
                        match msg {
                            bollard::container::LogOutput::StdOut { message } => {
                                stdout.extend_from_slice(&message);
                            }
                            bollard::container::LogOutput::StdErr { message } => {
                                stderr.extend_from_slice(&message);
                            }
                            _ => {}
                        }
                    }

                    let mut combined_output = stdout;
                    combined_output.extend_from_slice(&stderr);
                    String::from_utf8_lossy(&combined_output).to_string()
                };

                let output = if let Some(timeout_duration) = timeout_secs {
                    match timeout(Duration::from_secs(timeout_duration), collect_output).await {
                        Ok(output) => output,
                        Err(_) => {
                            warn!("Command execution timed out after {} seconds", timeout_duration);
                            return Err(anyhow!("Command execution timed out").into());
                        }
                    }
                } else {
                    collect_output.await
                };

                // Get exit code
                let exec_inspect = self
                    .client
                    .inspect_exec(&exec.id)
                    .await
                    .context("Failed to inspect exec")?;

                let exit_code = exec_inspect.exit_code.unwrap_or(-1) as i64;
                (output, exit_code)
            }
            Ok(StartExecResults::Detached) => {
                return Err(anyhow!("Unexpected detached exec result").into());
            }
            Err(e) => {
                return Err(anyhow!("Failed to start exec: {}", e).into());
            }
        };

        debug!("Command executed with exit code: {}", exit_code);
        Ok((output, exit_code))
    }

    /// Copy a file or directory from local system to container
    pub async fn copy_to_container(
        &self,
        container_id: &str,
        source_path: &Path,
        dest_path: &Path,
    ) -> DgmResult<()> {
        if !source_path.exists() {
            return Err(anyhow!("Source path does not exist: {:?}", source_path).into());
        }

        info!("Copying {:?} to container at {:?}", source_path, dest_path);

        // Create destination directory in container
        let dest_dir = dest_path.parent().unwrap_or_else(|| Path::new("/"));
        self.exec_command(
            container_id,
            &["mkdir", "-p", &dest_dir.to_string_lossy()],
            Some(30),
        )
        .await?;

        // Create tar archive
        let tar_data = self.create_file_archive(source_path, dest_path).await?;

        // Upload to container
        let options = UploadToContainerOptions {
            path: dest_dir.to_string_lossy().to_string(),
            ..Default::default()
        };

        let body = Either::Left(Full::new(Bytes::from(tar_data)));
        self.client
            .upload_to_container(container_id, Some(options), body)
            .await
            .context("Failed to upload to container")?;

        info!("Successfully copied to container");
        Ok(())
    }

    /// Copy a file or directory from container to local system
    pub async fn copy_from_container(
        &self,
        container_id: &str,
        source_path: &Path,
        dest_path: &Path,
    ) -> DgmResult<()> {
        info!("Copying from container {:?} to local {:?}", source_path, dest_path);

        // Check if source exists in container
        let (_, exit_code) = self
            .exec_command(
                container_id,
                &["test", "-e", &source_path.to_string_lossy()],
                Some(30),
            )
            .await?;

        if exit_code != 0 {
            return Err(anyhow!("Source path does not exist in container: {:?}", source_path).into());
        }

        // Create destination directory
        if let Some(parent) = dest_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create destination directory")?;
        }

        // Download from container
        let options = DownloadFromContainerOptions {
            path: source_path.to_string_lossy().to_string(),
            ..Default::default()
        };
        let mut stream = self
            .client
            .download_from_container(container_id, Some(options));

        let mut tar_data = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read chunk from container")?;
            tar_data.extend_from_slice(&chunk);
        }

        // Extract tar archive
        self.extract_archive(&tar_data, dest_path).await?;

        info!("Successfully copied from container");
        Ok(())
    }

    /// Create a tar archive for a single file or directory
    async fn create_file_archive(&self, source_path: &Path, dest_path: &Path) -> DgmResult<Vec<u8>> {
        let mut tar_data = Vec::new();
        {
            let mut tar = Builder::new(&mut tar_data);

            if source_path.is_file() {
                // Add single file
                let file_name = dest_path
                    .file_name()
                    .ok_or_else(|| anyhow!("Invalid destination file name"))?;
                tar.append_path_with_name(source_path, file_name)
                    .context("Failed to add file to tar")?;
            } else {
                // Add directory recursively
                tar.append_dir_all(dest_path.file_name().unwrap_or_default(), source_path)
                    .context("Failed to add directory to tar")?;
            }

            tar.finish().context("Failed to finalize tar archive")?;
        }

        Ok(tar_data)
    }

    /// Extract tar archive to destination path
    async fn extract_archive(&self, tar_data: &[u8], dest_path: &Path) -> DgmResult<()> {
        let dest_path = dest_path.to_path_buf();
        let tar_data = tar_data.to_vec();

        // Run the extraction in a blocking task to avoid Send issues
        tokio::task::spawn_blocking(move || {
            let cursor = Cursor::new(tar_data);
            let mut archive = tar::Archive::new(cursor);

            if dest_path.extension().is_some() {
                // Extracting to a specific file
                let entries = archive.entries().context("Failed to read tar entries")?;
                for entry in entries {
                    let mut entry = entry.context("Failed to read tar entry")?;

                    let mut buffer = Vec::new();
                    std::io::Read::read_to_end(&mut entry, &mut buffer)
                        .context("Failed to read entry data")?;

                    std::fs::write(&dest_path, &buffer)
                        .context("Failed to write file data")?;
                    break; // Only extract first entry for single file
                }
            } else {
                // Extracting to a directory
                archive
                    .unpack(&dest_path)
                    .context("Failed to extract tar archive")?;
            }

            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("Failed to spawn blocking task")?
        .context("Failed to extract archive")?;

        Ok(())
    }

    /// Remove a Docker image
    pub async fn remove_image(&self, image_name: &str, force: bool) -> DgmResult<()> {
        let options = RemoveImageOptions {
            force,
            ..Default::default()
        };

        self.client
            .remove_image(image_name, Some(options), None)
            .await
            .context("Failed to remove image")?;

        info!("Image '{}' removed successfully", image_name);
        Ok(())
    }

    /// Write string data to a file in container
    pub async fn write_to_container(
        &self,
        container_id: &str,
        data: &str,
        dest_path: &Path,
    ) -> DgmResult<()> {
        let heredoc_delimiter = "EOF_DGM_RUST";
        let command = format!(
            "cat <<'{}' > {}\n{}\n{}",
            heredoc_delimiter,
            dest_path.to_string_lossy(),
            data,
            heredoc_delimiter
        );

        self.exec_command(container_id, &["sh", "-c", &command], Some(60))
            .await?;

        info!("Successfully wrote data to container file: {:?}", dest_path);
        Ok(())
    }

    /// Get container information
    pub async fn inspect_container(&self, container_id: &str) -> DgmResult<ContainerInspectResponse> {
        self.client
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
            .context("Failed to inspect container")
            .map_err(Into::into)
    }
}
