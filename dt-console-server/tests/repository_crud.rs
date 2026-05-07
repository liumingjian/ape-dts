//! Integration tests for repository CRUD operations against :memory: SQLite.

use dt_console_server::db;
use dt_console_server::models::*;
use dt_console_server::repositories::alarm_channel_repository::AlarmChannelRepository;
use dt_console_server::repositories::alarm_template_repository::AlarmTemplateRepository;
use dt_console_server::repositories::alert_repository::AlertRepository;
use dt_console_server::repositories::alert_rule_repository::AlertRuleRepository;
use dt_console_server::repositories::control_log_repository::ControlLogRepository;
use dt_console_server::repositories::global_param_repository::GlobalParamRepository;
use dt_console_server::repositories::license_repository::LicenseRepository;
use dt_console_server::repositories::metric_point_repository::MetricPointRepository;
use dt_console_server::repositories::operate_log_repository::OperateLogRepository;
use dt_console_server::repositories::resource_group_repository::ResourceGroupRepository;
use dt_console_server::repositories::run_repository::RunRepository;
use dt_console_server::repositories::session_repository::SessionRepository;
use dt_console_server::repositories::system_host_repository::SystemHostRepository;
use dt_console_server::repositories::task_repository::TaskRepository;
use dt_console_server::repositories::user_repository::UserRepository;
use sqlx::SqlitePool;

/// Helper: create a migrated test pool backed by a temp file.
async fn test_pool() -> SqlitePool {
    let dir = std::env::temp_dir().join("dt-console-server-repo-test");
    std::fs::create_dir_all(&dir).unwrap();
    let test_name = std::thread::current()
        .name()
        .unwrap_or("unknown")
        .to_string();
    let safe_name: String = test_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let path = dir.join(format!("repo-{safe_name}.db"));
    let _ = std::fs::remove_file(&path);
    let path_str = path.to_string_lossy().to_string();
    let pool = db::create_pool(&path_str).await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    pool
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

// ─── UserRepository ──────────────────────────────────────────────────────

#[tokio::test]
async fn user_repository_crud() {
    let pool = test_pool().await;

    let user = User {
        id: uuid::Uuid::new_v4().to_string(),
        username: "admin".to_string(),
        password_hash: "$2b$10$hash".to_string(),
        display_name: "Admin".to_string(),
        role: "admin".to_string(),
        disabled: false,
        resource_group_id: None,
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = UserRepository::create(&pool, &user).await.unwrap();
    assert_eq!(created.username, "admin");
    assert_eq!(created.role, "admin");

    // Find by id
    let found = UserRepository::find_by_id(&pool, &user.id).await.unwrap();
    assert_eq!(found.username, "admin");

    // Find by username
    let by_name = UserRepository::find_by_username(&pool, "admin")
        .await
        .unwrap();
    assert_eq!(by_name.id, user.id);

    // List
    let list = UserRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = found.clone();
    updated.display_name = "Super Admin".to_string();
    updated.role = "operator".to_string();
    let saved = UserRepository::update(&pool, &updated).await.unwrap();
    assert_eq!(saved.display_name, "Super Admin");
    assert_eq!(saved.role, "operator");

    // Count by role
    let count = UserRepository::count_by_role(&pool, "operator")
        .await
        .unwrap();
    assert_eq!(count, 1);

    // Delete
    UserRepository::delete(&pool, &user.id).await.unwrap();
    assert!(UserRepository::find_by_id(&pool, &user.id).await.is_err());
}

#[tokio::test]
async fn user_repository_duplicate_username_rejected() {
    let pool = test_pool().await;

    let user1 = User {
        id: uuid::Uuid::new_v4().to_string(),
        username: "alice".to_string(),
        password_hash: "hash1".to_string(),
        display_name: "Alice".to_string(),
        role: "viewer".to_string(),
        disabled: false,
        resource_group_id: None,
        created_at: now(),
        updated_at: now(),
    };

    let mut user2 = user1.clone();
    user2.id = uuid::Uuid::new_v4().to_string();
    user2.display_name = "Alice 2".to_string();

    UserRepository::create(&pool, &user1).await.unwrap();
    let result = UserRepository::create(&pool, &user2).await;
    assert!(result.is_err(), "duplicate username should be rejected");
}

// ─── SessionRepository ──────────────────────────────────────────────────

#[tokio::test]
async fn session_repository_crud() {
    let pool = test_pool().await;

    // Create a user first (FK constraint).
    let user = User {
        id: uuid::Uuid::new_v4().to_string(),
        username: "testuser".to_string(),
        password_hash: "hash".to_string(),
        display_name: "Test".to_string(),
        role: "admin".to_string(),
        disabled: false,
        resource_group_id: None,
        created_at: now(),
        updated_at: now(),
    };
    UserRepository::create(&pool, &user).await.unwrap();

    let session = Session {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: user.id.clone(),
        token: "session-token-123".to_string(),
        created_at: now(),
        expires_at: None,
        ip: Some("127.0.0.1".to_string()),
        user_agent: Some("test-agent".to_string()),
    };

    // Create
    let created = SessionRepository::create(&pool, &session).await.unwrap();
    assert_eq!(created.token, "session-token-123");

    // Find by token
    let by_token = SessionRepository::find_by_token(&pool, "session-token-123")
        .await
        .unwrap();
    assert_eq!(by_token.user_id, user.id);

    // Delete by id (logout)
    SessionRepository::delete(&pool, &session.id).await.unwrap();
    assert!(SessionRepository::find_by_id(&pool, &session.id)
        .await
        .is_err());

    // Delete by user
    let session2 = Session {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: user.id.clone(),
        token: "session-token-456".to_string(),
        created_at: now(),
        expires_at: None,
        ip: None,
        user_agent: None,
    };
    SessionRepository::create(&pool, &session2).await.unwrap();
    SessionRepository::delete_by_user(&pool, &user.id)
        .await
        .unwrap();
    assert!(SessionRepository::find_by_token(&pool, "session-token-456")
        .await
        .is_err());
}

// ─── ResourceGroupRepository ─────────────────────────────────────────────

#[tokio::test]
async fn resource_group_repository_crud() {
    let pool = test_pool().await;

    let rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "default".to_string(),
        is_default: true,
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = ResourceGroupRepository::create(&pool, &rg).await.unwrap();
    assert_eq!(created.name, "default");
    assert!(created.is_default);

    // List
    let list = ResourceGroupRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = created.clone();
    updated.name = "production".to_string();
    let saved = ResourceGroupRepository::update(&pool, &updated)
        .await
        .unwrap();
    assert_eq!(saved.name, "production");

    // Delete
    ResourceGroupRepository::delete(&pool, &rg.id)
        .await
        .unwrap();
    assert!(ResourceGroupRepository::find_by_id(&pool, &rg.id)
        .await
        .is_err());
}

// ─── TaskRepository ──────────────────────────────────────────────────────

#[tokio::test]
async fn task_repository_crud() {
    let pool = test_pool().await;

    // Create a resource group and user first (FK constraints).
    let rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "default".to_string(),
        is_default: true,
        created_at: now(),
        updated_at: now(),
    };
    ResourceGroupRepository::create(&pool, &rg).await.unwrap();

    let user = User {
        id: uuid::Uuid::new_v4().to_string(),
        username: "taskowner".to_string(),
        password_hash: "hash".to_string(),
        display_name: "Owner".to_string(),
        role: "admin".to_string(),
        disabled: false,
        resource_group_id: None,
        created_at: now(),
        updated_at: now(),
    };
    UserRepository::create(&pool, &user).await.unwrap();

    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: "snap_test_1".to_string(),
        name: "Test Snapshot".to_string(),
        kind: "snapshot".to_string(),
        db_type_source: "mysql".to_string(),
        db_type_target: "mysql".to_string(),
        source_endpoint: "{}".to_string(),
        target_endpoint: "{}".to_string(),
        extractor_config: "{}".to_string(),
        sinker_config: "{}".to_string(),
        filter_config: "{}".to_string(),
        router_config: "{}".to_string(),
        parallelizer_config: "{}".to_string(),
        pipeline_config: "{}".to_string(),
        resumer_config: "{}".to_string(),
        processor_config: "{}".to_string(),
        runtime_config: "{}".to_string(),
        metrics_config: "{}".to_string(),
        resource_group_id: rg.id.clone(),
        owner_user_id: Some(user.id.clone()),
        status: "draft".to_string(),
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = TaskRepository::create(&pool, &task).await.unwrap();
    assert_eq!(created.kind, "snapshot");
    assert_eq!(created.status, "draft");

    // Find by id
    let found = TaskRepository::find_by_id(&pool, &task.id).await.unwrap();
    assert_eq!(found.task_id, "snap_test_1");

    // List
    let list = TaskRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = found.clone();
    updated.status = "ready".to_string();
    let saved = TaskRepository::update(&pool, &updated).await.unwrap();
    assert_eq!(saved.status, "ready");

    // Count
    let count = TaskRepository::count(&pool).await.unwrap();
    assert_eq!(count, 1);

    // Delete
    TaskRepository::delete(&pool, &task.id).await.unwrap();
    assert!(TaskRepository::find_by_id(&pool, &task.id).await.is_err());
    let count_after = TaskRepository::count(&pool).await.unwrap();
    assert_eq!(count_after, 0);
}

// ─── RunRepository ───────────────────────────────────────────────────────

#[tokio::test]
async fn run_repository_crud() {
    let pool = test_pool().await;

    // Create prerequisites: RG + user + task.
    let rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "default".to_string(),
        is_default: true,
        created_at: now(),
        updated_at: now(),
    };
    ResourceGroupRepository::create(&pool, &rg).await.unwrap();

    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: "run_test_1".to_string(),
        name: "Run Test".to_string(),
        kind: "snapshot".to_string(),
        db_type_source: "mysql".to_string(),
        db_type_target: "mysql".to_string(),
        source_endpoint: "{}".to_string(),
        target_endpoint: "{}".to_string(),
        extractor_config: "{}".to_string(),
        sinker_config: "{}".to_string(),
        filter_config: "{}".to_string(),
        router_config: "{}".to_string(),
        parallelizer_config: "{}".to_string(),
        pipeline_config: "{}".to_string(),
        resumer_config: "{}".to_string(),
        processor_config: "{}".to_string(),
        runtime_config: "{}".to_string(),
        metrics_config: "{}".to_string(),
        resource_group_id: rg.id.clone(),
        owner_user_id: None,
        status: "draft".to_string(),
        created_at: now(),
        updated_at: now(),
    };
    TaskRepository::create(&pool, &task).await.unwrap();

    let run = Run {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task.id.clone(),
        status: "running".to_string(),
        pid: Some(12345),
        ini_path: Some("/tmp/ini".to_string()),
        log_dir: Some("/tmp/logs".to_string()),
        started_at: Some(now()),
        stopped_at: None,
        exit_code: None,
        stop_method: None,
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = RunRepository::create(&pool, &run).await.unwrap();
    assert_eq!(created.status, "running");

    // Find by id
    let found = RunRepository::find_by_id(&pool, &run.id).await.unwrap();
    assert_eq!(found.pid, Some(12345));

    // List by task
    let list = RunRepository::list_by_task(&pool, &task.id).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = found.clone();
    updated.status = "stopped".to_string();
    updated.exit_code = Some(0);
    let saved = RunRepository::update(&pool, &updated).await.unwrap();
    assert_eq!(saved.status, "stopped");
    assert_eq!(saved.exit_code, Some(0));
}

// ─── LicenseRepository ──────────────────────────────────────────────────

#[tokio::test]
async fn license_repository_crud() {
    let pool = test_pool().await;

    let license = License {
        id: uuid::Uuid::new_v4().to_string(),
        sku: "professional".to_string(),
        max_tasks: 10,
        expire_at: Some("2027-01-01T00:00:00Z".to_string()),
        activated_at: Some(now()),
        activation_code_hash: Some("hashed_code".to_string()),
        granted_to: "acme-corp".to_string(),
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = LicenseRepository::create(&pool, &license).await.unwrap();
    assert_eq!(created.sku, "professional");
    assert_eq!(created.max_tasks, 10);

    // Get current
    let current = LicenseRepository::get_current(&pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.id, license.id);

    // Update
    let mut updated = created.clone();
    updated.max_tasks = 20;
    let saved = LicenseRepository::update(&pool, &updated).await.unwrap();
    assert_eq!(saved.max_tasks, 20);
}

// ─── AlertRepository ─────────────────────────────────────────────────────

#[tokio::test]
async fn alert_repository_crud() {
    let pool = test_pool().await;

    let alert = Alert {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: None,
        run_id: None,
        rule_id: None,
        metric_name: Some("extractor_rps_avg".to_string()),
        operator: Some(">".to_string()),
        threshold: Some(100.0),
        severity: "warning".to_string(),
        value: Some(150.0),
        status: "firing".to_string(),
        fired_at: now(),
        recovered_at: None,
        cleared_at: None,
        created_at: now(),
    };

    // Create
    let created = AlertRepository::create(&pool, &alert).await.unwrap();
    assert_eq!(created.status, "firing");
    assert_eq!(created.severity, "warning");

    // Find by id
    let found = AlertRepository::find_by_id(&pool, &alert.id).await.unwrap();
    assert_eq!(found.metric_name, Some("extractor_rps_avg".to_string()));

    // List
    let list = AlertRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update (recover alert)
    let mut updated = found.clone();
    updated.status = "recovered".to_string();
    updated.recovered_at = Some(now());
    let saved = AlertRepository::update(&pool, &updated).await.unwrap();
    assert_eq!(saved.status, "recovered");
}

// ─── AlertRuleRepository ─────────────────────────────────────────────────

#[tokio::test]
async fn alert_rule_repository_crud() {
    let pool = test_pool().await;

    let rule = AlertRule {
        id: uuid::Uuid::new_v4().to_string(),
        name: "High RPS".to_string(),
        metric_name: "extractor_rps_avg".to_string(),
        operator: ">".to_string(),
        threshold: 100.0,
        severity: "warning".to_string(),
        dwell_secs: 30,
        enabled: true,
        resource_group_id: None,
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = AlertRuleRepository::create(&pool, &rule).await.unwrap();
    assert_eq!(created.name, "High RPS");

    // List
    let list = AlertRuleRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = created.clone();
    updated.threshold = 200.0;
    updated.enabled = false;
    let saved = AlertRuleRepository::update(&pool, &updated).await.unwrap();
    assert_eq!(saved.threshold, 200.0);
    assert!(!saved.enabled);

    // Delete
    AlertRuleRepository::delete(&pool, &rule.id).await.unwrap();
    assert!(AlertRuleRepository::find_by_id(&pool, &rule.id)
        .await
        .is_err());
}

// ─── AlarmChannelRepository ──────────────────────────────────────────────

#[tokio::test]
async fn alarm_channel_repository_crud() {
    let pool = test_pool().await;

    let channel = AlarmChannel {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Kafka Alert".to_string(),
        kind: "kafka".to_string(),
        config: r#"{"topic":"alerts","brokers":"localhost:9092"}"#.to_string(),
        enabled: true,
        resource_group_id: None,
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = AlarmChannelRepository::create(&pool, &channel)
        .await
        .unwrap();
    assert_eq!(created.kind, "kafka");

    // List
    let list = AlarmChannelRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = created.clone();
    updated.kind = "snmp".to_string();
    updated.config = r#"{"host":"192.168.1.1","community":"public"}"#.to_string();
    let saved = AlarmChannelRepository::update(&pool, &updated)
        .await
        .unwrap();
    assert_eq!(saved.kind, "snmp");

    // Delete
    AlarmChannelRepository::delete(&pool, &channel.id)
        .await
        .unwrap();
}

// ─── AlarmTemplateRepository ─────────────────────────────────────────────

#[tokio::test]
async fn alarm_template_repository_crud() {
    let pool = test_pool().await;

    let template = AlarmTemplate {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Default Alert".to_string(),
        subject_template: "[{{severity}}] {{metric_name}} alert".to_string(),
        body_template: "Metric {{metric_name}} {{operator}} {{threshold}}".to_string(),
        severity_mapping: r#"{"warning":"info","critical":"urgent"}"#.to_string(),
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = AlarmTemplateRepository::create(&pool, &template)
        .await
        .unwrap();
    assert_eq!(created.name, "Default Alert");

    // List
    let list = AlarmTemplateRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = created.clone();
    updated.subject_template = "ALERT: {{metric_name}}".to_string();
    let saved = AlarmTemplateRepository::update(&pool, &updated)
        .await
        .unwrap();
    assert_eq!(saved.subject_template, "ALERT: {{metric_name}}");

    // Delete
    AlarmTemplateRepository::delete(&pool, &template.id)
        .await
        .unwrap();
}

// ─── OperateLogRepository ───────────────────────────────────────────────

#[tokio::test]
async fn operate_log_repository_crud() {
    let pool = test_pool().await;

    let log = OperateLog {
        id: 0, // auto-increment
        actor: "admin".to_string(),
        action: "auth.login".to_string(),
        result: "success".to_string(),
        target: None,
        details: Some(r#"{"ip":"127.0.0.1"}"#.to_string()),
        ip: Some("127.0.0.1".to_string()),
        created_at: now(),
    };

    // Create
    let created = OperateLogRepository::create(&pool, &log).await.unwrap();
    assert!(created.id > 0, "auto-increment id should be assigned");
    assert_eq!(created.actor, "admin");
    assert_eq!(created.action, "auth.login");

    // List
    let list = OperateLogRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);
}

// ─── ControlLogRepository ────────────────────────────────────────────────

#[tokio::test]
async fn control_log_repository_crud() {
    let pool = test_pool().await;

    // Create prerequisites: RG + task.
    let rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "default".to_string(),
        is_default: true,
        created_at: now(),
        updated_at: now(),
    };
    ResourceGroupRepository::create(&pool, &rg).await.unwrap();

    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: "ctrl_test_1".to_string(),
        name: "Ctrl Test".to_string(),
        kind: "snapshot".to_string(),
        db_type_source: "mysql".to_string(),
        db_type_target: "mysql".to_string(),
        source_endpoint: "{}".to_string(),
        target_endpoint: "{}".to_string(),
        extractor_config: "{}".to_string(),
        sinker_config: "{}".to_string(),
        filter_config: "{}".to_string(),
        router_config: "{}".to_string(),
        parallelizer_config: "{}".to_string(),
        pipeline_config: "{}".to_string(),
        resumer_config: "{}".to_string(),
        processor_config: "{}".to_string(),
        runtime_config: "{}".to_string(),
        metrics_config: "{}".to_string(),
        resource_group_id: rg.id.clone(),
        owner_user_id: None,
        status: "running".to_string(),
        created_at: now(),
        updated_at: now(),
    };
    TaskRepository::create(&pool, &task).await.unwrap();

    // Write intent row.
    let intent = ControlLog {
        id: 0,
        task_id: task.id.clone(),
        run_id: None,
        action: "start".to_string(),
        intent_or_result: "intent".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: now(),
    };
    let created = ControlLogRepository::create(&pool, &intent).await.unwrap();
    assert!(created.id > 0);
    assert_eq!(created.intent_or_result, "intent");

    // Write result row.
    let result = ControlLog {
        id: 0,
        task_id: task.id.clone(),
        run_id: None,
        action: "start".to_string(),
        intent_or_result: "result:success".to_string(),
        operator_id: Some("admin".to_string()),
        created_at: now(),
    };
    let created_result = ControlLogRepository::create(&pool, &result).await.unwrap();
    assert_eq!(created_result.intent_or_result, "result:success");

    // List
    let list = ControlLogRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 2);
}

// ─── GlobalParamRepository ───────────────────────────────────────────────

#[tokio::test]
async fn global_param_repository_crud() {
    let pool = test_pool().await;

    let gp = GlobalParam {
        id: uuid::Uuid::new_v4().to_string(),
        key: "idle_timeout_secs".to_string(),
        value: "3600".to_string(),
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = GlobalParamRepository::create(&pool, &gp).await.unwrap();
    assert_eq!(created.key, "idle_timeout_secs");

    // Find by key
    let by_key = GlobalParamRepository::find_by_key(&pool, "idle_timeout_secs")
        .await
        .unwrap();
    assert_eq!(by_key.value, "3600");

    // List
    let list = GlobalParamRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = created.clone();
    updated.value = "7200".to_string();
    let saved = GlobalParamRepository::update(&pool, &updated)
        .await
        .unwrap();
    assert_eq!(saved.value, "7200");
}

// ─── MetricPointRepository ───────────────────────────────────────────────

#[tokio::test]
async fn metric_point_repository_crud() {
    let pool = test_pool().await;

    // Create prerequisites: RG + task.
    let rg = ResourceGroup {
        id: uuid::Uuid::new_v4().to_string(),
        name: "default".to_string(),
        is_default: true,
        created_at: now(),
        updated_at: now(),
    };
    ResourceGroupRepository::create(&pool, &rg).await.unwrap();

    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: "metric_test_1".to_string(),
        name: "Metric Test".to_string(),
        kind: "snapshot".to_string(),
        db_type_source: "mysql".to_string(),
        db_type_target: "mysql".to_string(),
        source_endpoint: "{}".to_string(),
        target_endpoint: "{}".to_string(),
        extractor_config: "{}".to_string(),
        sinker_config: "{}".to_string(),
        filter_config: "{}".to_string(),
        router_config: "{}".to_string(),
        parallelizer_config: "{}".to_string(),
        pipeline_config: "{}".to_string(),
        resumer_config: "{}".to_string(),
        processor_config: "{}".to_string(),
        runtime_config: "{}".to_string(),
        metrics_config: "{}".to_string(),
        resource_group_id: rg.id.clone(),
        owner_user_id: None,
        status: "running".to_string(),
        created_at: now(),
        updated_at: now(),
    };
    TaskRepository::create(&pool, &task).await.unwrap();

    let mp = MetricPoint {
        id: 0,
        task_id: task.id.clone(),
        run_id: "run-1".to_string(),
        metric_name: "extractor_rps_avg".to_string(),
        ts: now(),
        value: 42.5,
    };

    // Create
    let created = MetricPointRepository::create(&pool, &mp).await.unwrap();
    assert!(created.id > 0);
    assert_eq!(created.metric_name, "extractor_rps_avg");

    // List by run
    let list = MetricPointRepository::list_by_run(&pool, "run-1")
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].value, 42.5);
}

// ─── SystemHostRepository ───────────────────────────────────────────────

#[tokio::test]
async fn system_host_repository_crud() {
    let pool = test_pool().await;

    let host = SystemHost {
        id: uuid::Uuid::new_v4().to_string(),
        hostname: "worker-1".to_string(),
        ip: "10.0.0.1".to_string(),
        status: "healthy".to_string(),
        last_heartbeat: Some(now()),
        created_at: now(),
        updated_at: now(),
    };

    // Create
    let created = SystemHostRepository::create(&pool, &host).await.unwrap();
    assert_eq!(created.hostname, "worker-1");

    // List
    let list = SystemHostRepository::list(&pool).await.unwrap();
    assert_eq!(list.len(), 1);

    // Update
    let mut updated = created.clone();
    updated.status = "unhealthy".to_string();
    let saved = SystemHostRepository::update(&pool, &updated).await.unwrap();
    assert_eq!(saved.status, "unhealthy");

    // Delete
    SystemHostRepository::delete(&pool, &host.id).await.unwrap();
    assert!(SystemHostRepository::find_by_id(&pool, &host.id)
        .await
        .is_err());
}
