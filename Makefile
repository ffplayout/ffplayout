SHELL := /bin/bash
current_dir = $(shell pwd)

init:
	virtualenv -p python3 venv
	source ./venv/bin/activate && pip install -r requirements.txt

	@echo ""
	@echo "-------------------------------------------------------------------"
	@echo "external packages for ffplayout installed in \"$(current_dir)/venv\""
	@echo ""
	@echo "run: \"$(current_dir)/venv/bin/python\" \"$(current_dir)/ffplayout.py\""
	@echo ""
	@echo "or:"
	@echo "source ./venv/bin/activate"
	@echo "./ffplayout.py"
	@echo ""
