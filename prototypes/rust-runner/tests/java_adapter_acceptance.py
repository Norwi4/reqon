"""Run Java/Rust parity and server adapter tests against disposable local fixtures.

Requires a test Kafka broker (explicit --broker) and a Java checkout with the adapter.
Every DB write uses a UUID schema in the loopback fixture cluster, never a stand DB.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import uuid
from kafka import KafkaProducer, KafkaConsumer, TopicPartition
from kafka.admin import KafkaAdminClient, NewTopic
from kafka.structs import OffsetAndMetadata
from psycopg import sql
from fixture import WorkflowServer
from service_fixture import PostgresStore
from test_e2e import BINARY


def cases(topic):
    create = {'service': 'Workflow', 'method': 'create', 'request': {'name': 'isolated'}, 'save': {'id': 'id'}}
    get = {'service': 'poc.Workflow', 'method': 'Get', 'request': {'id': '${id}'},
           'poll': {'interval': '10ms', 'timeout': '2s', 'until': {'equalTo': {'status': 'CREATED'}}},
           'expect': {'equalTo': {'count': '3'}, 'notEmpty': ['id']}, 'save': {'count': 'count'}}
    echo = {'service': 'poc.Workflow', 'method': 'Echo', 'request': {'payload': 'count=${count}'},
            'expect': {'equalTo': {'payload': 'count=3'}}}
    return [
        ('duration-forms', 'PASSED', [{'pause': '0ms'}, {'pause': 'PT0.001S'}, {'pause': '2'}]),
        ('target-case', 'PASSED', [{'service': 'poc.Workflow', 'method': 'Echo', 'target': ' LOCAL ', 'expect': {'equalTo': {'count': 7}}}]),
        ('missing-target-fallback', 'PASSED', [{'service': 'poc.Workflow', 'method': 'Echo', 'target': 'unknown', 'expect': {'equalTo': {'count': 7}}}]),
        ('chain', 'PASSED', [create, {'pause': '5ms'}, get, echo]),
        ('business-error', 'PASSED', [{'service': 'poc.Workflow', 'method': 'BusinessError', 'expect': {'success': False, 'errorCode': 'INVALID_INPUT'}}]),
        ('transport-error', 'PASSED', [{'service': 'poc.Workflow', 'method': 'TransportError', 'expect': {'success': False, 'errorCode': 'UNAVAILABLE'}}]),
        ('invalid-request', 'PASSED', [{'service': 'poc.Workflow', 'method': 'Echo', 'request': {'unknown': 1}, 'expect': {'success': False, 'errorCode': 'REQUEST_FORMAT_ERROR'}}]),
        ('streaming', 'PASSED', [{'service': 'poc.Workflow', 'method': 'Watch', 'expect': {'success': False, 'errorCode': 'UNSUPPORTED_METHOD'}}]),
        ('assertion-failure', 'FAILED', [{'service': 'poc.Workflow', 'method': 'Echo', 'expect': {'equalTo': {'count': 99}}}, {'pause': '1ms'}]),
        ('missing-save', 'FAILED', [{'service': 'poc.Workflow', 'method': 'Echo', 'save': {'v': 'missing'}}]),
        ('poll-continue', 'PASSED', [{'service': 'poc.Workflow', 'method': 'Echo', 'poll': {'interval': '10ms', 'timeout': '30ms', 'onTimeout': 'continue', 'until': {'equalTo': {'count': 99}}}, 'save': {'count': 'count'}}]),
        ('poll-fail', 'FAILED', [{'service': 'poc.Workflow', 'method': 'Echo', 'poll': {'interval': '10ms', 'timeout': '30ms', 'until': {'equalTo': {'count': 99}}}}]),
        ('kafka-before-action', 'PASSED', [create, {'pause': '30ms'}, {'kafka': {'topic': topic, 'timeout': '3s', 'match': {'id': '${id}'}, 'expect': {'equalTo': {'kind': 'created'}}, 'save': {'eventId': 'id'}}}]),
        ('kafka-from-step', 'PASSED', [create, {'pause': '500ms'}, {'kafka': {'topic': topic, 'timeout': '200ms', 'from': 'step', 'absent': True, 'match': {'id': '${id}'}}}]),
        ('kafka-unexpected', 'FAILED', [create, {'kafka': {'topic': topic, 'timeout': '2s', 'absent': True, 'match': {'id': '${id}'}}}]),
        ('kafka-missing', 'FAILED', [{'kafka': {'topic': topic, 'timeout': '200ms', 'match': {'id': 'does-not-exist'}}}]),
    ]


def main():
    sys.stdout.reconfigure(encoding='utf8', errors='replace')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--java', type=Path, required=True)
    parser.add_argument('--broker', required=True)
    args = parser.parse_args()
    if not args.broker.startswith('127.0.0.1:'):
        raise ValueError('This acceptance script only permits a local test broker')
    topic = 'reqon-parity-' + uuid.uuid4().hex
    admin = KafkaAdminClient(bootstrap_servers=args.broker, request_timeout_ms=15000)
    admin.create_topics([NewTopic(topic, num_partitions=2, replication_factor=1)])
    producer = KafkaProducer(bootstrap_servers=args.broker, value_serializer=lambda v: json.dumps(v).encode())
    group = 'reqon-other-' + uuid.uuid4().hex
    consumer = KafkaConsumer(bootstrap_servers=args.broker, group_id=group, enable_auto_commit=False)
    partitions = [TopicPartition(topic, i) for i in range(2)]
    consumer.assign(partitions)
    consumer.commit({p: OffsetAndMetadata(0, '', -1) for p in partitions})
    before = {p: consumer.committed(p) for p in partitions}
    store = PostgresStore()
    history = 'reqon_history_' + uuid.uuid4().hex
    store.connection.execute(sql.SQL('CREATE SCHEMA {}').format(sql.Identifier(history)))
    server = WorkflowServer(store=store, java_profile=True,
                            on_create=lambda value: producer.send(topic, value).get(timeout=10))
    result_code = 2
    try:
        with tempfile.TemporaryDirectory(prefix='reqon-parity-') as folder:
            root = Path(folder)
            documents = []
            for name, expected, steps in cases(topic):
                documents.append({'expected': expected, 'scenario': {'id': name, 'name': name, 'domain': 'local', 'steps': steps}})
            (root / 'cases.json').write_text(json.dumps(documents), encoding='utf8')
            environment = os.environ.copy()
            environment.update(REQON_TEST_BINARY=str(BINARY.resolve()), REQON_TEST_ENDPOINT=server.endpoint,
                               REQON_TEST_KAFKA=args.broker, REQON_TEST_CASES=str(root / 'cases.json'),
                               REQON_TEST_JDBC=f'jdbc:postgresql://127.0.0.1:55489/postgres?currentSchema={history}',
                               REQON_TEST_DB_USER='reqon_fixture')
            log = args.java / 'target/reqon-integration.log'
            log.parent.mkdir(exist_ok=True)
            with log.open('w', encoding='utf8') as output:
                run = subprocess.run([shutil.which('mvn') or 'mvn', '-o', '-q', '-Dtest=ReqonRunnerIntegrationTest', 'test'],
                                     cwd=args.java, env=environment, stdout=output, stderr=subprocess.STDOUT, timeout=240)
            result_code = run.returncode
            after = {p: consumer.committed(p) for p in partitions}
            assert after == before, f'Foreign offsets changed: {before} -> {after}'
            summary = {'java_exit': result_code, 'cases': len(documents), 'foreign_offsets_unchanged': True,
                       'fixture_rows_created': store.count(), 'broker': args.broker, 'log': str(log)}
            target = Path(__file__).resolve().parents[1] / 'results/java-parity-v0.13.0.json'
            target.write_text(json.dumps(summary, indent=2), encoding='utf8')
            print(json.dumps(summary, indent=2))
            if result_code:
                print(log.read_text(encoding='utf8', errors='replace')[-9000:])
    finally:
        server.close()
        store.connection.execute(sql.SQL('DROP SCHEMA {} CASCADE').format(sql.Identifier(history)))
        store.close()
        consumer.close(); producer.close()
        admin.delete_topics([topic]); admin.close()
    return result_code


if __name__ == '__main__':
    raise SystemExit(main())
