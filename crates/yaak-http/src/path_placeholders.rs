use yaak_models::models::HttpUrlParameter;

pub fn apply_path_placeholders(
    url: &str,
    parameters: &Vec<HttpUrlParameter>,
) -> (String, Vec<HttpUrlParameter>) {
    let mut new_parameters = Vec::new();

    let mut url = url.to_string();
    for p in parameters {
        if !p.enabled || p.name.is_empty() {
            continue;
        }

        // Replace path parameters with values from URL parameters
        let old_url_string = url.clone();
        url = replace_path_placeholder(&p, url.as_str());

        // Remove as param if it modified the URL
        if old_url_string == *url {
            new_parameters.push(p.to_owned());
        }
    }

    (url, new_parameters)
}

fn replace_path_placeholder(p: &HttpUrlParameter, url: &str) -> String {
    if !p.enabled {
        return url.to_string();
    }

    // Path-placeholder parameters have a `{name}`-shaped name.
    if !(p.name.starts_with('{') && p.name.ends_with('}') && p.name.len() > 2) {
        return url.to_string();
    }

    let encoded = urlencoding::encode(p.value.as_str()).to_string();
    url.replace(p.name.as_str(), encoded.as_str())
}

#[cfg(test)]
mod placeholder_tests {
    use crate::path_placeholders::{apply_path_placeholders, replace_path_placeholder};
    use yaak_models::models::{HttpRequest, HttpUrlParameter};

    #[test]
    fn placeholder_middle() {
        let p = HttpUrlParameter {
            name: "{foo}".into(),
            value: "xxx".into(),
            enabled: true,
            id: None,
        };
        assert_eq!(
            replace_path_placeholder(&p, "https://example.com/{foo}/bar"),
            "https://example.com/xxx/bar",
        );
    }

    #[test]
    fn placeholder_end() {
        let p = HttpUrlParameter {
            name: "{foo}".into(),
            value: "xxx".into(),
            enabled: true,
            id: None,
        };
        assert_eq!(
            replace_path_placeholder(&p, "https://example.com/{foo}"),
            "https://example.com/xxx",
        );
    }

    #[test]
    fn placeholder_with_literal_colon_suffix() {
        // The motivating OpenAPI case: brace-delimited placeholders can sit
        // next to literal `:` characters in a single path segment.
        let p = HttpUrlParameter {
            name: "{id}".into(),
            value: "42".into(),
            enabled: true,
            id: None,
        };
        assert_eq!(
            replace_path_placeholder(&p, "https://example.com/tasks/{id}:increment-importance"),
            "https://example.com/tasks/42:increment-importance",
        );
    }

    #[test]
    fn placeholder_multiple_occurrences() {
        let p = HttpUrlParameter {
            name: "{foo}".into(),
            value: "xxx".into(),
            enabled: true,
            id: None,
        };
        assert_eq!(
            replace_path_placeholder(&p, "https://example.com/{foo}/bar/{foo}"),
            "https://example.com/xxx/bar/xxx",
        );
    }

    #[test]
    fn placeholder_missing() {
        let p = HttpUrlParameter {
            enabled: true,
            name: "".to_string(),
            value: "".to_string(),
            id: None,
        };
        assert_eq!(
            replace_path_placeholder(&p, "https://example.com/{missing}"),
            "https://example.com/{missing}",
        );
    }

    #[test]
    fn placeholder_disabled() {
        let p = HttpUrlParameter {
            enabled: false,
            name: "{foo}".to_string(),
            value: "xxx".to_string(),
            id: None,
        };
        assert_eq!(
            replace_path_placeholder(&p, "https://example.com/{foo}"),
            "https://example.com/{foo}",
        );
    }

    #[test]
    fn placeholder_non_brace_name_left_alone() {
        // A parameter without `{...}` framing is a query parameter, not a
        // path placeholder, and must not touch the URL string.
        let p = HttpUrlParameter {
            name: "foo".into(),
            value: "xxx".into(),
            enabled: true,
            id: None,
        };
        assert_eq!(
            replace_path_placeholder(&p, "https://example.com/{foo}"),
            "https://example.com/{foo}",
        );
    }

    #[test]
    fn placeholder_encode() {
        let p = HttpUrlParameter {
            name: "{foo}".into(),
            value: "Hello World".into(),
            enabled: true,
            id: None,
        };
        assert_eq!(
            replace_path_placeholder(&p, "https://example.com/{foo}"),
            "https://example.com/Hello%20World",
        );
    }

    #[test]
    fn apply_placeholder() {
        let req = HttpRequest {
            url: "example.com/{a}/bar".to_string(),
            url_parameters: vec![
                HttpUrlParameter {
                    name: "b".to_string(),
                    value: "bbb".to_string(),
                    enabled: true,
                    id: None,
                },
                HttpUrlParameter {
                    name: "{a}".to_string(),
                    value: "aaa".to_string(),
                    enabled: true,
                    id: None,
                },
            ],
            ..Default::default()
        };

        let (url, url_parameters) = apply_path_placeholders(&req.url, &req.url_parameters);

        // Pattern match back to access it
        assert_eq!(url, "example.com/aaa/bar");
        assert_eq!(url_parameters.len(), 1);
        assert_eq!(url_parameters[0].name, "b");
        assert_eq!(url_parameters[0].value, "bbb");
    }
}
