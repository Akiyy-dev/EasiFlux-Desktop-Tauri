use crate::models::news::NewsStatusKind;
use crate::services::news::status::{default_message, empty_snapshot};

#[test]
fn every_status_kind_has_the_exact_safe_default_message() {
    let cases = [
        (
            NewsStatusKind::DeploymentMisconfigured,
            "新闻数据源未内置，请使用正确的安装包",
            &[
                0x65B0, 0x95FB, 0x6570, 0x636E, 0x6E90, 0x672A, 0x5185, 0x7F6E, 0xFF0C, 0x8BF7,
                0x4F7F, 0x7528, 0x6B63, 0x786E, 0x7684, 0x5B89, 0x88C5, 0x5305,
            ][..],
        ),
        (
            NewsStatusKind::NotConfigured,
            "新闻服务未配置",
            &[0x65B0, 0x95FB, 0x670D, 0x52A1, 0x672A, 0x914D, 0x7F6E],
        ),
        (
            NewsStatusKind::CredentialStoreUnavailable,
            "凭据存储暂不可用",
            &[
                0x51ED, 0x636E, 0x5B58, 0x50A8, 0x6682, 0x4E0D, 0x53EF, 0x7528,
            ],
        ),
        (
            NewsStatusKind::InitialSync,
            "正在同步历史新闻",
            &[
                0x6B63, 0x5728, 0x540C, 0x6B65, 0x5386, 0x53F2, 0x65B0, 0x95FB,
            ],
        ),
        (NewsStatusKind::Live, "实时", &[0x5B9E, 0x65F6]),
        (
            NewsStatusKind::Retrying,
            "新闻服务暂时不可用，正在重试",
            &[
                0x65B0, 0x95FB, 0x670D, 0x52A1, 0x6682, 0x65F6, 0x4E0D, 0x53EF, 0x7528, 0xFF0C,
                0x6B63, 0x5728, 0x91CD, 0x8BD5,
            ],
        ),
        (
            NewsStatusKind::CredentialInvalid,
            "新闻服务凭据无效",
            &[
                0x65B0, 0x95FB, 0x670D, 0x52A1, 0x51ED, 0x636E, 0x65E0, 0x6548,
            ],
        ),
        (
            NewsStatusKind::ContractError,
            "新闻服务协议异常",
            &[
                0x65B0, 0x95FB, 0x670D, 0x52A1, 0x534F, 0x8BAE, 0x5F02, 0x5E38,
            ],
        ),
        (
            NewsStatusKind::StorageError,
            "新闻缓存暂不可用",
            &[
                0x65B0, 0x95FB, 0x7F13, 0x5B58, 0x6682, 0x4E0D, 0x53EF, 0x7528,
            ],
        ),
        (
            NewsStatusKind::Stopped,
            "新闻服务已停止",
            &[0x65B0, 0x95FB, 0x670D, 0x52A1, 0x5DF2, 0x505C, 0x6B62],
        ),
    ];

    for (kind, expected, code_points) in cases {
        assert_eq!(default_message(kind), expected);
        assert_eq!(
            default_message(kind)
                .chars()
                .map(u32::from)
                .collect::<Vec<_>>(),
            code_points
        );
        let snapshot = empty_snapshot(kind);
        assert_eq!(snapshot.kind, kind);
        assert_eq!(snapshot.message.as_deref(), Some(expected));
        assert_eq!(snapshot.synced_count, 0);
        assert_eq!(snapshot.latest_delivery_id, None);
        assert_eq!(snapshot.unread_count, 0);
        assert_eq!(snapshot.retry_at, None);
    }
}
