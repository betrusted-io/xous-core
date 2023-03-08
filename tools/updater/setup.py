# -*- coding: utf-8 -*-
# precursorupdater

from setuptools import setup, find_packages
with open('README.md') as f:
    long_description = f.read()

setup(
    name="precursorupdater",
    version="0.1.2",
    author="bunnie",
    description="Precursor USB Updater",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/betrusted-io/betrusted-wiki/wiki/Updating-Your-Device",
    project_urls={
        "Bug Tracker": "https://github.com/betrusted-io/xous-core/issues",
        "Documentation": "https://github.com/betrusted-io/betrusted-wiki/wiki/Updating-Your-Device",
        "Source Code": "https://github.com/betrusted-io/xous-core/tree/main/tools/updater"
    },
    license="Apache2.0",
    classifiers=[
        "Programming Language :: Python :: 3",
        "License :: OSI Approved :: Apache Software License",
        "Operating System :: OS Independent",
        "Development Status :: 3 - Alpha",
        "Environment :: Console",
        "Topic :: System :: Hardware",
        "Topic :: System :: Hardware :: Universal Serial Bus (USB)",
    ],
    packages=find_packages(),
    install_requires=[
        "requests >= 2",
        "pyusb >= 1",
        "progressbar2 >= 3",
        "pycryptodome >= 3",
    ],
    entry_points="""
        [console_scripts]
        precursorupdater=precursorupdater:main
    """,
)
