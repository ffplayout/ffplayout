SHELL := /bin/bash
current_dir = $(shell pwd)

init:
	virtualenv -p python3 venv
	source ./venv/bin/activate && pip install -r requirements.txt

	@echo ""
	@echo "-------------------------------------------------------------------"
	@echo "packages for ffplayout installed in \"$(current_dir)/venv\""
	@echo ""
	@echo "run \"$(current_dir)/venv/bin/python\" \"$(current_dir)/ffplayout.py\""
