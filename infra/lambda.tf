data "archive_file" "api" {
  type        = "zip"
  source_file = "${path.module}/../dist/api/bootstrap"
  output_path = "${path.module}/../dist/api.zip"
}

data "archive_file" "durable" {
  type        = "zip"
  source_file = "${path.module}/../dist/durable/bootstrap"
  output_path = "${path.module}/../dist/durable.zip"
}

locals {
  env = {
    TABLE_NAME              = aws_dynamodb_table.store.name
    CART_TTL_SECONDS        = tostring(var.cart_ttl_seconds)
    PIX_TTL_SECONDS         = tostring(var.pix_ttl_seconds)
    CARD_DELAY_SECONDS      = tostring(var.card_delay_seconds)
    CARD_DELAY_LONG_SECONDS = tostring(var.card_delay_long_seconds)
    ALLOWED_ORIGIN          = "*"
    DURABLE_FUNCTION_NAME   = "${aws_lambda_function.durable.function_name}:live"
  }
}

resource "aws_lambda_function" "api" {
  function_name    = "${var.project_name}-${var.environment}-api"
  role             = aws_iam_role.api.arn
  filename         = data.archive_file.api.output_path
  source_code_hash = data.archive_file.api.output_base64sha256
  handler          = "bootstrap"
  runtime          = "provided.al2023"
  architectures    = ["arm64"]
  timeout          = 30
  memory_size      = 512
  publish          = true

  environment {
    variables = local.env
  }

  depends_on = [
    aws_iam_role_policy.api,
    aws_lambda_alias.durable,
  ]
}

resource "aws_lambda_alias" "api" {
  name             = "live"
  function_name    = aws_lambda_function.api.function_name
  function_version = aws_lambda_function.api.version
}

resource "aws_lambda_function" "durable" {
  function_name    = "${var.project_name}-${var.environment}-durable"
  role             = aws_iam_role.durable.arn
  filename         = data.archive_file.durable.output_path
  source_code_hash = data.archive_file.durable.output_base64sha256
  handler          = "bootstrap"
  runtime          = "provided.al2023"
  architectures    = ["arm64"]
  timeout          = 60
  memory_size      = 512
  publish          = true

  durable_config {
    execution_timeout = 3600
    retention_period  = 7
  }

  environment {
    variables = {
      TABLE_NAME              = aws_dynamodb_table.store.name
      CART_TTL_SECONDS        = tostring(var.cart_ttl_seconds)
      PIX_TTL_SECONDS         = tostring(var.pix_ttl_seconds)
      CARD_DELAY_SECONDS      = tostring(var.card_delay_seconds)
      CARD_DELAY_LONG_SECONDS = tostring(var.card_delay_long_seconds)
    }
  }

  timeouts {
    delete = "60m"
  }

  depends_on = [
    aws_iam_role_policy.durable,
    aws_iam_role_policy_attachment.durable_execution,
  ]
}

resource "aws_lambda_alias" "durable" {
  name             = "live"
  function_name    = aws_lambda_function.durable.function_name
  function_version = aws_lambda_function.durable.version
}
