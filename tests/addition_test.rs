mod common;
use common::setup_instance;
use mpc_exploration::{Config, Peer, routes::addition::LastSumResponse};
use tracing::Level;

#[tokio::test]
async fn test_addition() {
    let peer_1 = Peer::new(1, "http://localhost:50001".to_string());
    let peer_2 = Peer::new(2, "http://localhost:50002".to_string());
    let peer_3 = Peer::new(3, "http://localhost:50003".to_string());

    let config_1 = Config {
        port: 50001,
        log_level: Level::WARN,
        server_peer_id: 1,
        peers: vec![peer_2.clone(), peer_3.clone()],
    };
    let instance_1 = setup_instance(config_1).await.unwrap();
    let config_2 = Config {
        port: 50002,
        log_level: Level::WARN,
        server_peer_id: 2,
        peers: vec![peer_1.clone(), peer_3.clone()],
    };
    let instance_2 = setup_instance(config_2).await.unwrap();
    let config_3 = Config {
        port: 50003,
        log_level: Level::WARN,
        server_peer_id: 3,
        peers: vec![peer_1.clone(), peer_2.clone()],
    };
    let instance_3 = setup_instance(config_3).await.unwrap();

    let client = reqwest::Client::new();

    let send_share_1 = client
        .post(format!("{}/addition/send-share", &instance_1.server_url))
        .send()
        .await
        .unwrap();
    assert!(send_share_1.status().is_success());

    let send_share_2 = client
        .post(format!("{}/addition/send-share", &instance_2.server_url))
        .send()
        .await
        .unwrap();
    assert!(send_share_2.status().is_success());

    let send_share_3 = client
        .post(format!("{}/addition/send-share", &instance_3.server_url))
        .send()
        .await
        .unwrap();
    assert!(send_share_3.status().is_success());

    let send_sum_share_1 = client
        .post(format!(
            "{}/addition/send-sum-share",
            &instance_1.server_url
        ))
        .send()
        .await
        .unwrap();
    assert!(send_sum_share_1.status().is_success());

    let send_sum_share_2 = client
        .post(format!(
            "{}/addition/send-sum-share",
            &instance_2.server_url
        ))
        .send()
        .await
        .unwrap();
    assert!(send_sum_share_2.status().is_success());

    let send_sum_share_3 = client
        .post(format!(
            "{}/addition/send-sum-share",
            &instance_3.server_url
        ))
        .send()
        .await
        .unwrap();
    assert!(send_sum_share_3.status().is_success());

    let last_sum_1 = client
        .get(format!("{}/addition/last-sum", &instance_1.server_url))
        .send()
        .await
        .unwrap();
    assert!(last_sum_1.status().is_success());
    let last_sum_value_1 = last_sum_1.json::<LastSumResponse>().await.unwrap();
    assert!(last_sum_value_1.sum > 0);

    let last_sum_2 = client
        .get(format!("{}/addition/last-sum", &instance_2.server_url))
        .send()
        .await
        .unwrap();
    assert!(last_sum_2.status().is_success());
    let last_sum_value_2 = last_sum_2.json::<LastSumResponse>().await.unwrap();
    assert!(last_sum_value_2.sum > 0);

    let last_sum_3 = client
        .get(format!("{}/addition/last-sum", &instance_3.server_url))
        .send()
        .await
        .unwrap();
    assert!(last_sum_3.status().is_success());
    let last_sum_value_3 = last_sum_3.json::<LastSumResponse>().await.unwrap();
    assert!(last_sum_value_3.sum > 0);
}
