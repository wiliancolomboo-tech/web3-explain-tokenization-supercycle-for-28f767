export RESEND_API_KEY="re_1234567890abcdef"
export DISCORD_SLACK_WEBHOOK_URL="https://api.example.com/webhooks/alerts/placeholder"

npm install
npm run build
npm run agent originate-pool -- --senior 100000000000 --junior 250000000000 --duration 15552000