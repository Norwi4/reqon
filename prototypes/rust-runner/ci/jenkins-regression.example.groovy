// REQON_BIN points to a verified 0.14.1 executable from the release artifacts.
// TEST_TOKEN is a Jenkins credential referenced by the public Reqon environment.
stage('Reqon regression') {
  try {
    catchError(buildResult: 'SUCCESS', stageResult: 'UNSTABLE', catchInterruptions: false) {
        withCredentials([string(credentialsId: 'regression-test-token', variable: 'TEST_TOKEN')]) {
            sh '''
                "$REQON_BIN" validate regression/catalog.json --env environments/ci.json --json
                "$REQON_BIN" run regression/catalog.json --env environments/ci.json \
                  --json artifacts/result.json --junit artifacts/junit.xml --html artifacts/report.html
            '''
        }
    }
  } finally {
    catchError(buildResult: 'SUCCESS', stageResult: 'UNSTABLE', catchInterruptions: false) {
      junit testResults: 'artifacts/junit.xml', allowEmptyResults: true, skipMarkingBuildUnstable: true
      archiveArtifacts artifacts: 'artifacts/**', allowEmptyArchive: true
    }
  }
}
