# Trademark usage and review notes

`db3-kubectl` is an independent community project. References to the InfluxDB® 3 Enterprise database describe the external software with which this plugin is intended to work; they do not identify the plugin's source or imply endorsement.[^trademark]

## Source and scope

These project-copy changes are based on the published [Guidelines for Using InfluxData Trademarks](https://www.influxdata.com/legal/guidelines-for-using-influxdata-trademarks/), specifically:

- **Proper Use — Adjectives and Titles:** keep `db3-kubectl` as the project title and use the database name descriptively, modifying a generic noun such as "database."
- **Proper Use — Trademark Symbol:** use the registered symbol with the protected word mark in descriptive prose.
- **Proper Use — Attribution/Disclaimer:** display ownership and non-endorsement attribution in the README footnote and root CLI help, not only in this document.
- **Improper Use — Implied Affiliation; Logos:** do not present this project as official, endorsed, certified, or sponsored, and do not add vendor logos or brand artwork without the required written license. One exception is recorded below and in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

The source was read through a Jina Reader rendering of the supplied URL after direct access was blocked. This is a scoped implementation review, not a legal opinion or confirmation that every use is permitted.

## Recorded exceptions

- **Kubernetes favicon.** The console favicon is the Kubernetes icon, taken unmodified from the CNCF artwork repository and used as an interoperability reference under the Linux Foundation trademark usage guidelines. This was an owner decision that knowingly sets aside the "required written license" condition in *Improper Use — Implied Affiliation; Logos* above; the non-endorsement requirement in that bullet still applies, and the icon is attributed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Revert `webui/index.html` to restore the previous mark if this exception is withdrawn.

## Contributor guidance

- Keep project branding distinct from third-party product branding. Do not introduce vendor marks into a new project, crate, domain, or distribution name without reviewing the policy and any required permission.
- Do not invent new abbreviations, combine marks, or copy slogans, logos, or "powered-by"/"built-on" artwork.
- Use descriptive compatibility wording and the correct symbol for any protected product name used in prose. Put a visible attribution/non-endorsement footnote on each new documentation page that uses the mark; a link to this document alone is not a substitute.
- Preserve literal interoperability identifiers: `influxdb3`, `INFLUXDB3_TOKEN`, `INFLUXDB3_AUTH_TOKEN`, Kubernetes resource/namespace examples, and external URLs. Do not insert trademark symbols into executable names, environment variables, flags, or wire formats.
- The project's software license does not grant rights to third-party trademarks.

## Unresolved permission and publication review

The guidelines include both a fair-use discussion and permission/sample-submission requirements. No written permission or logo license has been supplied for this review, and this change obtains none. These edits do not establish a fair-use determination, legal compliance, or approval from the trademark owner. The maintainer should resolve which permissions apply and obtain any required review before relying on such a conclusion; the source provides the contact and request process. No permission request is submitted by this change.

This PR changes source, package description, documentation, and future CLI builds only. It does not update existing GitHub repository descriptions, release notes, or published v0.1.0 archives. Those publication surfaces, existing account/project names, and any future distribution names remain separate review items. Existing artifacts must not be silently replaced under the same version.

[^trademark]: InfluxDB® is a trademark registered by InfluxData, which is not affiliated with, and does not endorse, this project.
