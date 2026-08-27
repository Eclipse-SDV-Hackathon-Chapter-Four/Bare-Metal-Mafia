from setuptools import find_packages, setup

package_name = "hack_to_the_future_hvac"

setup(
    name=package_name,
    version="0.1.0",
    packages=find_packages(exclude=["test"]),
    data_files=[
        ("share/ament_index/resource_index/packages", [f"resource/{package_name}"]),
        (f"share/{package_name}", ["package.xml"]),
        (f"share/{package_name}/launch", ["launch/muto.launch.py", "launch/hvac.launch.py"]),
        (f"share/{package_name}/config", ["config/hvac_stack.json", "config/muto.yaml"]),
    ],
    install_requires=["setuptools"],
    zip_safe=True,
    maintainer="Codex",
    maintainer_email="devnull@example.com",
    description="ROS 2 HVAC simulator stack for Hack to the Future.",
    license="EPL-2.0",
    entry_points={
        "console_scripts": [
            "hvac_simulator = hack_to_the_future_hvac.hvac_simulator:main",
            "deploy_stack = hack_to_the_future_hvac.deploy_stack:main",
        ],
    },
)
