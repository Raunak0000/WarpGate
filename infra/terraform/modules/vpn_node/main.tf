# VPC
resource "aws_vpc" "warpgate_vpc" {
  cidr_block           = var.vpc_cidr
  enable_dns_support   = true
  enable_dns_hostnames = true

  tags = {
    Name = "warpgate-vpc"
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
resource "aws_subnet" "warpgate_subnet" {
  vpc_id                  = aws_vpc.warpgate_vpc.id
  cidr_block              = var.subnet_cidr
  map_public_ip_on_launch = true

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
  description = "Allow WireGuard and SSH inbound traffic"
  vpc_id      = aws_vpc.warpgate_vpc.id

  ingress {
    description = "SSH"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"] # Consider restricting to your home IP later for security
  }

  ingress {
    description = "WireGuard"
    from_port   = 51820
    to_port     = 51820
    protocol    = "udp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  egress {
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
  owners      = ["099720109477"] # Canonical

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*"]
  }
}

# EC2 Instance
resource "aws_instance" "warpgate_node" {
  ami           = data.aws_ami.ubuntu.id
  instance_type = "t3.micro"
  subnet_id     = aws_subnet.warpgate_subnet.id

  key_name      = aws_key_pair.warpgate_key.key_name

  vpc_security_group_ids = [aws_security_group.warpgate_sg.id]

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