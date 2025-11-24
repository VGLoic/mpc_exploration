mod common;

use common::setup_instance;
use futures::{StreamExt, stream};
use mpc_exploration::{
    Config, Peer,
    routes::addition::{CreateProcessHttpBody, GetProcessResponse},
};
use tracing::Level;

#[tokio::test]
async fn test_addition_single_process() {
    let instances = setup_instances(&[50001, 50002, 50003]).await;

    let client = reqwest::Client::new();

    let process_id = uuid::Uuid::new_v4();
    // Start addition process on all instances
    for instance in &instances {
        let create_addition_process_response = client
            .post(format!("{}/additions", &instance.server_url))
            .json(&CreateProcessHttpBody { process_id })
            .send()
            .await
            .unwrap();
        assert!(create_addition_process_response.status().is_success());
    }

    assert_completed_addition_process(&client, &instances, process_id).await;
}

#[tokio::test]
async fn test_addition_multiple_process() {
    let instances = setup_instances(&[50004, 50005, 50006]).await;

    let client = reqwest::Client::new();

    let process_ids = (0..100).map(|_| uuid::Uuid::new_v4()).collect::<Vec<_>>();

    for process_id in &process_ids {
        // Start addition process on all instances
        for instance in &instances {
            let create_addition_process_response = client
                .post(format!("{}/additions", &instance.server_url))
                .json(&CreateProcessHttpBody {
                    process_id: *process_id,
                })
                .send()
                .await
                .unwrap();
            assert!(create_addition_process_response.status().is_success());
        }
    }
    for process_id in &process_ids {
        assert_completed_addition_process(&client, &instances, *process_id).await;
    }
}

async fn setup_instances(ports: &[u16]) -> Vec<common::InstanceState> {
    let peers = ports
        .iter()
        .enumerate()
        .map(|(i, port)| Peer::new((i + 1) as u8, format!("http://localhost:{}", port)))
        .collect::<Vec<_>>();

    let mut configs = Vec::new();
    for (i, port) in ports.iter().enumerate() {
        let peer_list = peers
            .iter()
            .filter(|p| p.id != (i + 1) as u8)
            .cloned()
            .collect::<Vec<_>>();
        let config = Config {
            port: port.clone(),
            log_level: Level::WARN,
            server_peer_id: (i + 1) as u8,
            peers: peer_list,
        };
        configs.push(config);
    }

    let mut instances = Vec::new();
    for config in configs {
        instances.push(setup_instance(config).await.unwrap());
    }
    instances
}

async fn assert_completed_addition_process(
    client: &reqwest::Client,
    instances: &[common::InstanceState],
    process_id: uuid::Uuid,
) {
    let wait_for_completion_bodies = stream::iter(instances)
        .map(|instance| async move {
            wait_for_completed_addition_process(client, instance, process_id).await
        })
        .buffer_unordered(3);
    let wait_for_completion_results: Vec<Result<CompletedAdditionProcess, anyhow::Error>> =
        wait_for_completion_bodies.collect().await;
    let wait_for_completion_results: Vec<CompletedAdditionProcess> = wait_for_completion_results
        .into_iter()
        .map(|res| res.unwrap())
        .collect();

    let expected_sum = (wait_for_completion_results
        .iter()
        .map(|res| Into::<u128>::into(res.input))
        .sum::<u128>()
        % 1_000_000_007) as u64;

    for (index, completed_process) in wait_for_completion_results.iter().enumerate() {
        assert_eq!(
            completed_process.sum,
            expected_sum,
            "Instance {} computed incorrect sum",
            index + 1
        );
    }
}

struct CompletedAdditionProcess {
    input: u64,
    sum: u64,
}
async fn wait_for_completed_addition_process(
    client: &reqwest::Client,
    instance: &common::InstanceState,
    process_id: uuid::Uuid,
) -> Result<CompletedAdditionProcess, anyhow::Error> {
    let mut safe_counter: i32 = 0;
    loop {
        if let Ok(process) = client
            .get(format!("{}/additions/{}", &instance.server_url, process_id))
            .send()
            .await?
            .json::<GetProcessResponse>()
            .await
            && let Some(sum) = process.sum
        {
            return Ok(CompletedAdditionProcess {
                input: process.input,
                sum,
            });
        } else {
            safe_counter += 1;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            if safe_counter >= 50 {
                return Err(anyhow::anyhow!("Addition process did not complete in time"));
            }
        }
    }
}
