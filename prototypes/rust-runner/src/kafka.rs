//! Read-only partition observers: snapshot offsets before actions, never join a
//! consumer group, commit offsets, create topics or write records.
use crate::{
    Problem, RunOptions, StepReport,
    compatibility::{self, KafkaWait},
};
use anyhow::{Context, Result, bail};
use rskafka::client::{
    ClientBuilder, Credentials, SaslConfig,
    partition::{OffsetAt, PartitionClient, UnknownTopicHandling},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bootstrap_servers: Vec<String>,
    pub tls: bool,
    pub sasl_mechanism: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub subscribe_timeout_ms: Option<u64>,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.bootstrap_servers.len() > 16
            || self
                .bootstrap_servers
                .iter()
                .any(|v| v.contains('/') || v.contains('@') || !v.contains(':'))
        {
            bail!("invalid Kafka bootstrap servers");
        }
        if self
            .sasl_mechanism
            .as_deref()
            .is_some_and(|v| !["PLAIN", "SCRAM-SHA-256", "SCRAM-SHA-512"].contains(&v))
        {
            bail!("unsupported Kafka SASL mechanism");
        }
        if self.sasl_mechanism.is_some() != (self.username.is_some() && self.password.is_some()) {
            bail!("Kafka SASL requires username and password secret reference");
        }
        if self
            .subscribe_timeout_ms
            .is_some_and(|v| v == 0 || v > 60_000)
        {
            bail!("invalid Kafka subscription timeout");
        }
        Ok(())
    }
}
struct Partition {
    client: PartitionClient,
    start: i64,
}
pub(crate) struct Subscription {
    topics: BTreeMap<String, Vec<Partition>>,
}
impl Subscription {
    pub async fn start(topics: BTreeSet<String>, options: &RunOptions) -> Result<Self> {
        let config = &options.context.kafka;
        if config.bootstrap_servers.is_empty() {
            bail!("KAFKA_NOT_CONFIGURED");
        }
        let transport = &options.context.transport;
        if !config.tls && !transport.config.allow_plaintext_remote {
            for address in &config.bootstrap_servers {
                crate::transport::url_allowed(&format!("http://{address}"), false)?;
            }
        }
        let mut builder = ClientBuilder::new(config.bootstrap_servers.clone())
            .client_id("reqon-observer")
            .max_message_size(8 * 1024 * 1024);
        if config.tls {
            use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
            let mut roots = rustls::RootCertStore::empty();
            for cert in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(cert);
            }
            if let Some(ca) = &transport.ca {
                for cert in CertificateDer::pem_slice_iter(ca) {
                    roots.add(cert?)?;
                }
            }
            let tls = rustls::ClientConfig::builder().with_root_certificates(roots);
            let tls = if let Some(identity) = &transport.identity {
                tls.with_client_auth_cert(
                    CertificateDer::pem_slice_iter(&identity.0).collect::<Result<Vec<_>, _>>()?,
                    PrivateKeyDer::from_pem_slice(&identity.1)?,
                )?
            } else {
                tls.with_no_client_auth()
            };
            builder = builder.tls_config(Arc::new(tls));
        }
        if let Some(mechanism) = &config.sasl_mechanism {
            let vars = options.context.initial_values();
            let credentials = Credentials::new(
                crate::spec::resolve_text(config.username.as_ref().unwrap(), &vars)?,
                crate::spec::resolve_text(config.password.as_ref().unwrap(), &vars)?,
            );
            builder = builder.sasl_config(match mechanism.as_str() {
                "PLAIN" => SaslConfig::Plain(credentials),
                "SCRAM-SHA-256" => SaslConfig::ScramSha256(credentials),
                _ => SaslConfig::ScramSha512(credentials),
            });
        }
        let work = async {
            let client = builder.build().await?;
            let metadata = client.list_topics().await?;
            let mut observed = BTreeMap::new();
            let mut count = 0;
            for name in topics {
                let topic = metadata
                    .iter()
                    .find(|t| t.name == name)
                    .context("Kafka topic does not exist or is not visible")?;
                if topic.partitions.is_empty() {
                    bail!("Kafka topic has no partitions");
                }
                let mut partitions = vec![];
                for id in &topic.partitions {
                    count += 1;
                    if count > 64 {
                        bail!("Kafka observation limited to 64 partitions");
                    }
                    let partition = client
                        .partition_client(name.clone(), *id, UnknownTopicHandling::Error)
                        .await?;
                    let start = partition.get_offset(OffsetAt::Latest).await?;
                    partitions.push(Partition {
                        client: partition,
                        start,
                    });
                }
                observed.insert(name, partitions);
            }
            Ok::<_, anyhow::Error>(Self { topics: observed })
        };
        tokio::time::timeout(
            Duration::from_millis(config.subscribe_timeout_ms.unwrap_or(15_000)),
            work,
        )
        .await
        .context("Kafka subscription timed out")?
    }

    pub async fn wait(
        &self,
        wait: &KafkaWait,
        result: &mut StepReport,
        vars: &mut crate::runtime::Variables,
        options: &RunOptions,
    ) -> Result<(), Problem> {
        let partitions = self
            .topics
            .get(&wait.topic)
            .context("Kafka topic was not prepared")?;
        let deadline =
            Instant::now() + Duration::from_millis(compatibility::duration(&wait.timeout)?);
        let mut offsets = vec![];
        for p in partitions {
            offsets.push(if wait.from == "step" {
                tokio::time::timeout(
                    deadline.saturating_duration_since(Instant::now()),
                    p.client.get_offset(OffsetAt::Latest),
                )
                .await
                .context("Kafka step offset deadline")?
                .map_err(anyhow::Error::from)?
            } else {
                p.start
            });
        }
        let matches = compatibility::substitute(
            &serde_json::to_value(&wait.matches).map_err(anyhow::Error::from)?,
            vars,
        )?;
        let mut scanned = 0usize;
        let mut bytes = 0usize;
        let mut first_round = true;
        loop {
            for (partition, offset) in partitions.iter().zip(&mut offsets) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() && !first_round {
                    break;
                }
                // Allow one bounded final broker round trip. A broken connection is never evidence of absence.
                let fetched = tokio::time::timeout(
                    remaining.max(Duration::from_secs(1)),
                    partition.client.fetch_records(*offset, 1..1024 * 1024, 25),
                )
                .await;
                let records = fetched
                    .context("Kafka fetch timed out; absence cannot be established")?
                    .map_err(anyhow::Error::from)?
                    .0;
                for record in records {
                    *offset = record.offset + 1;
                    scanned += 1;
                    bytes += record.record.approximate_size();
                    if scanned > 100_000 || bytes > 32 * 1024 * 1024 {
                        return Err(Problem::error("Kafka observation budget exceeded"));
                    }
                    let raw =
                        String::from_utf8_lossy(record.record.value.as_deref().unwrap_or_default());
                    let body = if wait.format == "string" {
                        Value::String(raw.to_string())
                    } else {
                        match serde_json::from_str::<Value>(&raw) {
                            Ok(v) => v,
                            Err(_) => continue,
                        }
                    };
                    let matched = matches.as_object().unwrap().iter().all(|(path, expected)| {
                        if wait.format == "string" {
                            raw.contains(&match expected {
                                Value::String(v) => v.clone(),
                                _ => expected.to_string(),
                            })
                        } else {
                            compatibility::find(&body, path)
                                .is_some_and(|v| compatibility::equal(expected, v))
                        }
                    });
                    if !matched {
                        continue;
                    }
                    result.response = Some(crate::capture::value(&body, 64 * 1024));
                    result.transport = Some(crate::TransportReport {
                        protocol: "kafka".into(),
                        response_bytes: raw.len(),
                        ..Default::default()
                    });
                    result.logs.push(format!(
                        "Kafka partition {}, offset {}",
                        partition.client.partition(),
                        record.offset
                    ));
                    if wait.absent {
                        return Err(Problem::failed("unexpected matching Kafka message"));
                    }
                    result.checks = compatibility::evaluate(&wait.expect, &body, None, vars)?;
                    if result.checks.iter().any(|c| !c.passed) {
                        return Err(Problem::failed("Kafka response assertion failed"));
                    }
                    return compatibility::save(&body, &wait.save, vars, options);
                }
            }
            first_round = false;
            if Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(
                Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
        }
        result.checks.push(crate::CheckResult {
            path: "$kafka".into(),
            expected: json!(!wait.absent),
            actual: Some(json!(false)),
            passed: wait.absent,
            operation: "message_received".into(),
            target: "body".into(),
        });
        if wait.absent {
            Ok(())
        } else {
            Err(Problem::failed(
                "matching Kafka message not received before deadline",
            ))
        }
    }
}
