variable "aws_profile" {
  type        = string
  description = "Optional AWS CLI profile. Empty uses the default credential chain (AWS_PROFILE, env, ~/.aws, or IAM role)."
  default     = ""
}

variable "aws_region" {
  type        = string
  description = "AWS region. Durable functions must be available in the chosen region."
  default     = "us-east-2"
}

variable "project_name" {
  type        = string
  description = "Prefixo dos recursos. Sem dados de conta."
  default     = "arena-rust"
}

variable "environment" {
  type    = string
  default = "demo"
}

variable "cart_ttl_seconds" {
  type        = number
  description = "Timeout do callback de checkout (reserva de carrinho)."
  default     = 900
}

variable "pix_ttl_seconds" {
  type        = number
  description = "Timeout do callback PIX."
  default     = 900
}

variable "card_delay_seconds" {
  type    = number
  default = 8
}

variable "card_delay_long_seconds" {
  type    = number
  default = 20
}
