-- 005: hub_event に retry / DLQ field を追加 (consumer poison pill 対策)
--
-- apply が恒久的に失敗する event が毎 tick 無限再試行されるのを防ぐ。
-- consumer は失敗時に retry_count を増やし、 閾値超過で failed_at を立てて
-- unprocessed から除外する (= dead-letter)。
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

DEFINE FIELD OVERWRITE retry_count ON hub_event TYPE int DEFAULT 0
  COMMENT 'apply 失敗回数';
DEFINE FIELD OVERWRITE failed_at ON hub_event TYPE option<datetime>
  COMMENT 'dead-letter 時刻 (retry 上限超過)。 NONE = まだ生きてる';
DEFINE FIELD OVERWRITE last_error ON hub_event TYPE option<string>
  COMMENT '直近の apply エラー文言 (診断用)';
