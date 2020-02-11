SHELL := /bin/bash
CURRENT_DIR = $(shell pwd)

init:
	virtualenv -p python3 venv
	source ./venv/bin/activate && pip install -r requirements.txt
	@echo ""
	@echo "-------------------------------------------------------------------"
	@echo "external packages for ffplayout installed in \"$(CURRENT_DIR)/venv\""
	@echo ""
	@echo "run:"
	@echo "\"$(CURRENT_DIR)/venv/bin/python\" \"$(CURRENT_DIR)/ffplayout.py\""
	@echo ""
	@echo "or:"
	@echo "source ./venv/bin/activate"
	@echo "./ffplayout.py"
	@echo ""
	@echo "-------------------------------------------------------------------"
	@echo "run \"sudo make install USER=www-data\" if you would like to run ffplayout on server like environments"
	@echo "instead of www-data you can use any user which need write access to the config file"
	@echo "this user will also be placed in systemd service"
	@echo "systemd is required!"

install:
	if [ ! "$(CURRENT_DIR)" == "/opt/ffplayout-engine" ]; then \
		install -d -o $(USER) -g $(USER) /opt/ffplayout-engine/; \
		cp -r docs ffplayout venv "/opt/ffplayout-engine/"; \
		chown $(USER):$(USER) -R "/opt/ffplayout-engine/"; \
		install -m 755 -o $(USER) -g $(USER) ffplayout.py "/opt/ffplayout-engine/"; \
	fi
	install -d /etc/ffplayout/
	install -d -o $(USER) -g $(USER) /var/log/ffplayout/
	if [ ! -f "/etc/ffplayout/ffplayout.yml" ]; then \
		install -m 644 -o $(USER) -g $(USER) ffplayout.yml /etc/ffplayout/; \
	fi
	if [ -d "/etc/systemd/system" ] && [ ! -f "/etc/systemd/system/ffplayout.service" ]; then \
		install -m 644 docs/ffplayout.service /etc/systemd/system/; \
		sed -i "s/root/$(USER)/g" "/etc/systemd/system/ffplayout.service"; \
	fi
	@echo ""
	@echo "-------------------------------------------------------------------"
	@echo "installation done..."
	@echo ""
	@echo "if you want ffplayout to autostart, run: \"systemctl enable ffplayout\""

clean:
	rm -rf venv

uninstall:
	rm -rf "/etc/ffplayout"
	rm -rf "/var/log/ffplayout"
	rm -rf "/etc/systemd/system/ffplayout.service"
	if [ ! "$(CURRENT_DIR)" == "/opt/ffplayout-engine" ]; then \
		rm -rf "/opt/ffplayout-engine"; \
	fi
