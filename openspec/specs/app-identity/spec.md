# app-identity Specification

## Purpose

Names the build the user is running and makes that name trustworthy in the release, so support and bug reports start from a known version.

## Requirements

### Requirement: Settings shows the build that is running
Settings SHALL show the app's version and the platform it is running on, taken from the build rather than typed into the view. The value SHALL be available offline and without opening another window.

#### Scenario: Settings identifies the build
- **WHEN** the user opens Settings
- **THEN** an About section shows `LogNotes <version>` and the platform

#### Scenario: The version follows the build
- **WHEN** the app is built as version 0.2.0
- **THEN** Settings shows 0.2.0, not a value hardcoded in the interface

#### Scenario: Offline
- **WHEN** the machine has no network
- **THEN** the version is still shown

### Requirement: The About section names the project
The About section SHALL identify LogNotes as a journal-first outliner on a local database and state the MIT licence.

#### Scenario: Project identity
- **WHEN** the user reads the About section
- **THEN** it says what the app is and that it is MIT licensed

### Requirement: A release matches the version it claims
A release SHALL NOT publish when its tag does not match the package version. The check SHALL fail before artifacts are built.

#### Scenario: Matching tag
- **WHEN** tag `v0.2.0` is pushed and the package version is `0.2.0`
- **THEN** the release build proceeds

#### Scenario: Mismatched tag
- **WHEN** tag `v0.2.0` is pushed and the package version is `0.1.0`
- **THEN** the release stops before building and names both values
