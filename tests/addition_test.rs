mod common;

use common::setup_instance;
use futures::{StreamExt, stream};
use mpc_exploration::{
    Config, Peer,
    routes::addition::{CreatedProcessResponse, GetProcessResponse},
};
use tracing::Level;

#[tokio::test]
async fn test_addition_single_process() {
    let log_level = Level::WARN;

    let peer_1 = Peer::new(1, "http://localhost:50001".to_string());
    let peer_2 = Peer::new(2, "http://localhost:50002".to_string());
    let peer_3 = Peer::new(3, "http://localhost:50003".to_string());

    let config_1 = Config {
        port: 50001,
        log_level,
        server_peer_id: 1,
        peers: vec![peer_2.clone(), peer_3.clone()],
    };
    let instance_1 = setup_instance(config_1).await.unwrap();
    let config_2 = Config {
        port: 50002,
        log_level,
        server_peer_id: 2,
        peers: vec![peer_1.clone(), peer_3.clone()],
    };
    let instance_2 = setup_instance(config_2).await.unwrap();
    let config_3 = Config {
        port: 50003,
        log_level,
        server_peer_id: 3,
        peers: vec![peer_1.clone(), peer_2.clone()],
    };
    let instance_3 = setup_instance(config_3).await.unwrap();

    let instances = vec![&instance_1, &instance_2, &instance_3];

    let client = reqwest::Client::new();

    // Start addition process on any instance
    let initial_instance_index = (rand::random::<u8>() as usize) % instances.len();
    let start_addition_response = client
        .post(format!(
            "{}/additions",
            &instances[initial_instance_index].server_url
        ))
        .send()
        .await
        .unwrap();
    assert!(start_addition_response.status().is_success());
    let process_id = start_addition_response
        .json::<CreatedProcessResponse>()
        .await
        .unwrap()
        .process_id;

    assert_completed_addition_process(&client, &instances, process_id).await;
}

async fn assert_completed_addition_process(
    client: &reqwest::Client,
    instances: &[&common::InstanceState],
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
