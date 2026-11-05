# 1. Configure secrets
mkdir -p secrets
echo "[1,2,3...]" > secrets/authority_key.json
echo "re_test_000000000000000000000000" > secrets/resend_api_key.txt
echo "https://api.example.com/webhooks/alerts/placeholder" > secrets/slack_webhook.txt

# 2. Spin up Dockerized daemon
docker compose up -d --build

# 3. Verify real-time healthcheck endpoint
curl -s http://localhost:8080/health | jq .