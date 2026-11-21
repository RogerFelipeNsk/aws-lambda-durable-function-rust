.PHONY: test test-rust build-lambda build-frontend local frontend-dev deploy fmt

export AWS_REGION ?= us-east-2

test: test-rust

test-rust:
	cd backend && cargo test

fmt:
	cd backend && cargo fmt

build-lambda:
	mkdir -p dist/api dist/durable
	cd backend && cargo lambda build --release --arm64 --bin api --bin durable
	cp backend/target/lambda/api/bootstrap dist/api/bootstrap
	cp backend/target/lambda/durable/bootstrap dist/durable/bootstrap

local:
	cd backend && cargo run --bin api

frontend-dev:
	cd frontend && npm install && npm run dev

build-frontend:
	cd frontend && npm install && npm run build

deploy: test-rust build-lambda
	cd infra && tofu init -input=false
	cd infra && tofu apply -input=false -auto-approve
	$(MAKE) build-frontend
	@bucket=$$(cd infra && tofu output -raw frontend_bucket); \
	dist=$$(cd infra && tofu output -raw cloudfront_distribution_id); \
	aws s3 sync frontend/dist "s3://$$bucket" --delete; \
	aws cloudfront create-invalidation --distribution-id "$$dist" --paths "/*" >/dev/null; \
	echo "Loja: $$(cd infra && tofu output -raw cloudfront_url)"
