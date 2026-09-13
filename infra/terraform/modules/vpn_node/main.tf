# VPC
resource "aws_vpc" "warpgate_vpc" {
  cidr_block           = var.vpc_cidr
  enable_dns_support   = true
  enable_dns_hostnames = true

  tags = {
    Name = "warpgate-vpc"
  }
}

# Default VPC security group
# The default security group should not allow unrestricted traffic.
resource "aws_default_security_group" "warpgate_default_sg" {
  vpc_id = aws_vpc.warpgate_vpc.id

  ingress = []
  egress  = []

  tags = {
    Name = "warpgate-default-sg"
  }
}

# Internet Gateway
resource "aws_internet_gateway" "warpgate_igw" {
  vpc_id = aws_vpc.warpgate_vpc.id

  tags = {
    Name = "warpgate-igw"
  }
}

# Public Subnet
#
# Public IP assignment is disabled because the VPN node uses
# an explicitly managed Elastic IP.
resource "aws_subnet" "warpgate_subnet" {
  vpc_id                  = aws_vpc.warpgate_vpc.id
  cidr_block              = var.subnet_cidr
  map_public_ip_on_launch = false

  tags = {
    Name = "warpgate-public-subnet"
  }
}

# Route Table
resource "aws_route_table" "warpgate_rt" {
  vpc_id = aws_vpc.warpgate_vpc.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.warpgate_igw.id
  }

  tags = {
    Name = "warpgate-rt"
  }
}

resource "aws_route_table_association" "warpgate_rta" {
  subnet_id      = aws_subnet.warpgate_subnet.id
  route_table_id = aws_route_table.warpgate_rt.id
}

# Security Group
resource "aws_security_group" "warpgate_sg" {
  name        = "warpgate-sg"
  description = "Security group for the WarpGate WireGuard VPN gateway"
  vpc_id      = aws_vpc.warpgate_vpc.id

  ingress {
    description = "SSH administrative access"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = [var.admin_cidr]
  }

  ingress {
    description = "WireGuard VPN clients"
    from_port   = 51820
    to_port     = 51820
    protocol    = "udp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  # WarpGate is a full-tunnel VPN gateway.
  # Clients may legitimately send arbitrary Internet traffic through
  # the EC2 instance, so unrestricted outbound traffic is intentional.
  #checkov:skip=CKV_AWS_382:Full-tunnel VPN gateway requires unrestricted outbound traffic
  egress {
    description = "Allow VPN gateway outbound Internet traffic"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name = "warpgate-sg"
  }
}

# SSH Key Pair
resource "aws_key_pair" "warpgate_key" {
  key_name   = "warpgate-key"
  public_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOfFNypsgLNoYhhTmLgWNne8hBnQKPUJ5wanjnXu7FD8 warpgate-admin"
}

# AMI Lookup (Ubuntu 22.04)
data "aws_ami" "ubuntu" {
  most_recent = true
  owners      = ["099720109477"]

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*"]
  }
}

# IAM Role for EC2
resource "aws_iam_role" "warpgate_ec2_role" {
  name = "warpgate-ec2-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"

    Statement = [
      {
        Effect = "Allow"

        Principal = {
          Service = "ec2.amazonaws.com"
        }

        Action = "sts:AssumeRole"
      }
    ]
  })

  tags = {
    Name = "warpgate-ec2-role"
  }
}

# Allow Systems Manager access for operational management.
resource "aws_iam_role_policy_attachment" "warpgate_ssm" {
  role       = aws_iam_role.warpgate_ec2_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

# EC2 Instance Profile
resource "aws_iam_instance_profile" "warpgate_profile" {
  name = "warpgate-ec2-profile"
  role = aws_iam_role.warpgate_ec2_role.name

  tags = {
    Name = "warpgate-ec2-profile"
  }
}

# EC2 Instance
resource "aws_instance" "warpgate_node" {
  ami           = data.aws_ami.ubuntu.id
  instance_type = "t3.micro"
  subnet_id     = aws_subnet.warpgate_subnet.id

  key_name = aws_key_pair.warpgate_key.key_name

  vpc_security_group_ids = [
    aws_security_group.warpgate_sg.id
  ]

  iam_instance_profile = aws_iam_instance_profile.warpgate_profile.name

  # Required because this instance forwards VPN traffic between
  # WireGuard and the Internet.
  source_dest_check = false

  # Enable CloudWatch detailed monitoring.
  monitoring = true

  # T3 instances support EBS optimization.
  ebs_optimized = true

  # Require IMDSv2.
  metadata_options {
    http_endpoint = "enabled"
    http_tokens   = "required"
  }

  # Encrypt the root EBS volume.
  root_block_device {
    encrypted = true
  }

  credit_specification {
    cpu_credits = "standard"
  }

  tags = {
    Name = "warpgate-node"
    Role = "vpn"
  }
}

# Elastic IP
resource "aws_eip" "warpgate_eip" {
  instance = aws_instance.warpgate_node.id
  domain   = "vpc"

  tags = {
    Name = "warpgate-eip"
  }
}

# KMS Key for CloudWatch Logs
#
# VPC Flow Logs may contain network metadata, so the CloudWatch
# log group is encrypted using a customer-managed KMS key.
resource "aws_kms_key" "warpgate_logs" {
  description         = "KMS key for WarpGate VPC flow logs"
  enable_key_rotation = true

  tags = {
    Name = "warpgate-logs-kms"
  }
}

# Explicit KMS key policy
#
# The account root retains administrative control of the key,
# while CloudWatch Logs is allowed to use the key for encryption.
data "aws_caller_identity" "current" {}

resource "aws_kms_key_policy" "warpgate_logs" {
  key_id = aws_kms_key.warpgate_logs.id

  policy = jsonencode({
    Version = "2012-10-17"

    Statement = [
      {
        Sid    = "EnableAccountRootPermissions"
        Effect = "Allow"

        Principal = {
          AWS = "arn:aws:iam::${data.aws_caller_identity.current.account_id}:root"
        }

        Action   = "kms:*"
        Resource = "*"
      },
      {
        Sid    = "AllowCloudWatchLogsUseOfKey"
        Effect = "Allow"

        Principal = {
          Service = "logs.amazonaws.com"
        }

        Action = [
          "kms:Encrypt",
          "kms:Decrypt",
          "kms:ReEncrypt*",
          "kms:GenerateDataKey*",
          "kms:DescribeKey"
        ]

        Resource = "*"
      }
    ]
  })
}

# KMS Alias
resource "aws_kms_alias" "warpgate_logs" {
  name          = "alias/warpgate-logs"
  target_key_id = aws_kms_key.warpgate_logs.key_id
}

# CloudWatch Log Group for VPC Flow Logs
resource "aws_cloudwatch_log_group" "warpgate_vpc_flow_logs" {
  name              = "/aws/vpc/warpgate"
  retention_in_days = 365
  kms_key_id        = aws_kms_key.warpgate_logs.arn

  tags = {
    Name = "warpgate-vpc-flow-logs"
  }
}

# IAM role used by VPC Flow Logs to publish to CloudWatch.
resource "aws_iam_role" "warpgate_flow_logs_role" {
  name = "warpgate-vpc-flow-logs-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"

    Statement = [
      {
        Effect = "Allow"

        Principal = {
          Service = "vpc-flow-logs.amazonaws.com"
        }

        Action = "sts:AssumeRole"
      }
    ]
  })

  tags = {
    Name = "warpgate-vpc-flow-logs-role"
  }
}

resource "aws_iam_role_policy" "warpgate_flow_logs_policy" {
  name = "warpgate-vpc-flow-logs-policy"
  role = aws_iam_role.warpgate_flow_logs_role.id

  policy = jsonencode({
    Version = "2012-10-17"

    Statement = [
      {
        Effect = "Allow"

        Action = [
          "logs:CreateLogStream",
          "logs:PutLogEvents"
        ]

        Resource = "${aws_cloudwatch_log_group.warpgate_vpc_flow_logs.arn}:*"
      }
    ]
  })
}

# VPC Flow Logs
resource "aws_flow_log" "warpgate_vpc_flow_log" {
  vpc_id = aws_vpc.warpgate_vpc.id

  traffic_type = "ALL"

  iam_role_arn    = aws_iam_role.warpgate_flow_logs_role.arn
  log_destination = aws_cloudwatch_log_group.warpgate_vpc_flow_logs.arn

  tags = {
    Name = "warpgate-vpc-flow-log"
  }
}