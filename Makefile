
# gdb:
# 	cargo fmt
# 	cargo rustc -- --cfg NOMANGLE
# 	$(call plant)

# normal:
# 	cargo fmt
# 	cargo build
# 	$(call plant)

# plant:
# 	sudo cp target/debug/lss /bin/
# 	sudo chmod 777 /bin/lss
# 	sudo mkdir -p /etc/lss
# 	sudo chown $(USER):$(USER) /etc/lss
# 	cp target/debug/lss ./ida_works/

all:
	cargo fmt
	cargo build
	sudo cp target/debug/lss /bin/
	sudo chmod 777 /bin/lss
	sudo mkdir -p /etc/lss
	sudo chown $(USER):$(USER) /etc/lss
	cp target/debug/lss ./ida_works/
