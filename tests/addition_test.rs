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

    for instance in [&instance_1, &instance_2, &instance_3] {
        let send_share = client
            .post(format!("{}/addition/send-share", &instance.server_url))
            .send()
            .await
            .unwrap();
        assert!(send_share.status().is_success());
    }

    for instance in [&instance_1, &instance_2, &instance_3] {
        let send_sum_share = client
            .post(format!("{}/addition/send-sum-share", &instance.server_url))
            .send()
            .await
            .unwrap();
        assert!(send_sum_share.status().is_success());
    }

    for instance in [&instance_1, &instance_2, &instance_3] {
        let last_sum = client
            .get(format!("{}/addition/last-sum", &instance.server_url))
            .send()
            .await
            .unwrap();
        assert!(last_sum.status().is_success());
        let last_sum_value = last_sum.json::<LastSumResponse>().await.unwrap();
        assert!(last_sum_value.sum > 0);
    }
}
