module "vpn_node" {
  source = "./modules/vpn_node"

  vpc_cidr    = var.vpc_cidr
  subnet_cidr = var.subnet_cidr
  admin_cidr  = var.admin_cidr
}
