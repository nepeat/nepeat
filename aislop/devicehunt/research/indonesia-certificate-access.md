# Indonesia public certificate search: access limitation

The public [issued-certificate page](https://sertifikasi.postel.go.id/sertifikat/sertifikat-terbit)
was inspected on September 26, 2026 (local date). Its published client performs
read-only GraphQL `licencefront` count/list queries at
`https://sertifikasi.postel.go.id/svc/master/query`. The public form separates
applicant, equipment name, trademark, model and marketing name; its default
filter is `berlaku = 1`.

One unauthenticated count request reproduced the form's Celestica trademark
filter and returned `errors: [{message: "forbidden", path: ["licencefront"]}]`
with `data: null`. The response is saved at
`research/source-access/postel-public-count-response.json`. Requests to that
endpoint stopped. No credentials, alternate headers, private queries, different
hosts or bypass routes were attempted. This is an access limitation, not a
zero-result search or evidence that Celestica has no certifications.

The device.report Amazon Data Services directory surfaced SDPPI-labelled
third-party switch models including AS7772-32X. Its directory grouping alone
does not establish Amazon as manufacturer, operator or applicant; its linked
68601/SDPPI/2020 page did not supply primary certificate fields. No such
relationship or certification assertion was imported. Future work needs an
authorized public export or independently published original certificate with
the relevant company roles stated explicitly.
