#![allow(dead_code)]
use crossterm::style::Stylize;
use promptuity::event::*;
use promptuity::{
    InputCursor, Prompt, PromptBody, PromptInput, PromptState, RenderPayload, Validator,
};

/// A trait for formatting the [`AutocompleteInput`] prompt.
pub trait AutocompleteFormatter {
    /// Formats the suggestions list.
    fn suggestions(&self, suggestions: &[String], selected_index: usize) -> String {
        suggestions
            .iter()
            .enumerate()
            .map(|(i, suggestion)| {
                if i == selected_index {
                    format!(" {} {}", "→".cyan(), suggestion.as_str().cyan().bold())
                } else {
                    format!("   {}", suggestion)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Formats the required error message.
    fn err_required(&self) -> String {
        "Required input".to_string()
    }
}

/// The default formatter for [`AutocompleteInput`].
#[derive(Default)]
pub struct DefaultAutocompleteFormatter;

impl DefaultAutocompleteFormatter {
    /// Creates a new [`DefaultAutocompleteFormatter`].
    pub fn new() -> Self {
        Self
    }
}

impl AutocompleteFormatter for DefaultAutocompleteFormatter {}

/// A trait for providing suggestions.
pub trait Suggester {
    /// Analyzes the input and returns suggestions if appropriate.
    /// This method handles both trigger detection and suggestion filtering.
    fn get_suggestions(&self, input: &str, cursor_position: usize) -> SuggestionContext;

    /// Formats the selected suggestion before it replaces the text in
    /// replace_range. Default implementation returns the suggestion as-is.
    fn format_suggestion(&self, suggestion: &str) -> String {
        suggestion.to_string()
    }
}

/// A suggester config that holds the suggestions list and
/// the trigger points for the suggestions and the action to be taken.
pub struct SuggesterConfig {
    pub suggestions: Vec<String>,
    pub trigger_chars: Vec<char>,
    pub submit_on_select: bool,
}

impl SuggesterConfig {
    /// Creates a new [`StaticSuggester`] with the given suggestions.
    pub fn new(suggestions: Vec<String>) -> Self {
        let mut unique_suggestions: Vec<_> = suggestions.into_iter().collect();
        unique_suggestions.sort();
        Self {
            suggestions: unique_suggestions,
            trigger_chars: vec![],
            submit_on_select: false,
        }
    }

    /// Sets the trigger characters that will activate suggestions
    pub fn with_triggers(mut self, triggers: Vec<char>) -> Self {
        self.trigger_chars = triggers;
        self
    }

    /// Sets whether selecting a suggestion should submit the input
    pub fn with_submit_on_select(mut self, submit: bool) -> Self {
        self.submit_on_select = submit;
        self
    }
}

/// A prompt for text input with autocomplete suggestions.
pub struct AutocompleteInput<S: Suggester> {
    formatter: Box<dyn AutocompleteFormatter>,
    suggester: Option<S>,
    message: String,
    hint: Option<String>,
    placeholder: Option<String>,
    required: bool,
    validator: Option<Box<dyn Validator<String>>>,
    input: InputCursor,
    suggestion_context: Option<SuggestionContext>,
    selected_index: usize,
}

impl<S: Suggester> AutocompleteInput<S> {
    /// Creates a new [`AutocompleteInput`] prompt.
    pub fn new(message: impl std::fmt::Display) -> Self {
        Self {
            formatter: Box::new(DefaultAutocompleteFormatter::new()),
            suggester: None,
            message: message.to_string(),
            hint: None,
            placeholder: None,
            required: true,
            validator: None,
            input: InputCursor::new(String::new(), 0),
            suggestion_context: None,
            selected_index: 0,
        }
    }
}

impl<S: Suggester> AutocompleteInput<S> {
    /// Sets the suggester for the prompt.
    pub fn with_suggester(mut self, suggester: S) -> Self {
        self.suggester = Some(suggester);
        self
    }

    /// Sets the hint message for the prompt.
    pub fn with_hint(mut self, hint: impl std::fmt::Display) -> Self {
        self.hint = Some(hint.to_string());
        self
    }

    /// Sets the placeholder message for the prompt.
    pub fn with_placeholder(mut self, placeholder: impl std::fmt::Display) -> Self {
        self.placeholder = Some(placeholder.to_string());
        self
    }

    fn update_suggestions(&mut self) {
        // if suggester is present, then ask for the suggestions.
        if let Some(suggester) = &self.suggester {
            let input = self.input.value();
            let cursor_pos = self.input.cursor();
            self.suggestion_context = Some(suggester.get_suggestions(&input, cursor_pos));
            self.selected_index = 0;
        }
    }

    fn select_suggestion(&mut self) {
        if let Some(suggester) = &self.suggester {
            if let Some(context) = &self.suggestion_context {
                if !context.suggestions.is_empty() && context.show_suggestions {
                    if let Some((start, end)) = context.replace_range {
                        let input = self.input.value();
                        let suggestion = suggester
                            .format_suggestion(&context.suggestions[self.selected_index])
                            .cyan()
                            .to_string();
                        let new_value =
                            format!("{}{}{}", &input[..start], suggestion, &input[end..]);
                        self.input = InputCursor::new(new_value, start + suggestion.len());
                    }
                }
            }
            self.suggestion_context = None;
        }
    }
}

impl<S: Suggester + 'static> AsMut<dyn Prompt<Output = String> + 'static> for AutocompleteInput<S> {
    fn as_mut(&mut self) -> &mut (dyn Prompt<Output = String> + 'static) {
        self
    }
}

impl<S: Suggester + 'static> AsMut<AutocompleteInput<S>> for AutocompleteInput<S> {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl<S: Suggester + 'static> Prompt for AutocompleteInput<S> {
    type Output = String;

    fn submit(&mut self) -> Self::Output {
        self.input.value()
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(validator) = &self.validator {
            validator.validate(&self.input.value())?;
        }
        Ok(())
    }

    fn handle(&mut self, code: KeyCode, modifiers: KeyModifiers) -> PromptState {
        match (code, modifiers) {
            (KeyCode::Esc, _) => {
                if self.suggestion_context.is_some() {
                    self.suggestion_context = None;
                    PromptState::Active
                } else {
                    PromptState::Cancel
                }
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => PromptState::Cancel,
            (KeyCode::Tab, _) | (KeyCode::Enter, _) => {
                if let Some(context) = &self.suggestion_context {
                    if !context.suggestions.is_empty() && context.show_suggestions {
                        let should_submit = context.submit_on_select;
                        self.select_suggestion();
                        if should_submit && self.validate().is_ok() {
                            return PromptState::Submit;
                        }
                        return PromptState::Active;
                    }
                }
                if code == KeyCode::Enter {
                    if self.input.is_empty() && self.required {
                        PromptState::Active
                    } else if self.validate().is_ok() {
                        PromptState::Submit
                    } else {
                        PromptState::Active
                    }
                } else {
                    PromptState::Active
                }
            }
            (KeyCode::Up, _) => {
                if let Some(context) = &mut self.suggestion_context {
                    if !context.suggestions.is_empty() {
                        self.selected_index = self.selected_index.saturating_sub(1);
                    }
                }
                PromptState::Active
            }
            (KeyCode::Down, _) => {
                if let Some(context) = &mut self.suggestion_context {
                    if !context.suggestions.is_empty() {
                        self.selected_index =
                            (self.selected_index + 1).min(context.suggestions.len() - 1);
                    }
                }
                PromptState::Active
            }
            (KeyCode::Left, _) | (KeyCode::Char('b'), KeyModifiers::CONTROL) => {
                self.input.move_left();
                PromptState::Active
            }
            (KeyCode::Right, _) | (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                self.input.move_right();
                PromptState::Active
            }
            (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.input.move_home();
                PromptState::Active
            }
            (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.input.move_end();
                PromptState::Active
            }
            (KeyCode::Backspace, _) | (KeyCode::Char('h'), KeyModifiers::CONTROL) => {
                self.input.delete_left_char();
                self.update_suggestions();
                PromptState::Active
            }
            (KeyCode::Delete, _) | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.input.delete_right_char();
                self.update_suggestions();
                PromptState::Active
            }
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.input.insert(c);
                self.update_suggestions();
                PromptState::Active
            }
            _ => PromptState::Active,
        }
    }

    fn render(&mut self, _: &PromptState) -> Result<RenderPayload, String> {
        let mut payload = RenderPayload::new(
            self.message.clone(),
            self.hint.clone(),
            self.placeholder.clone(),
        );

        payload = payload.input(PromptInput::Cursor(self.input.clone()));

        if let Some(context) = &self.suggestion_context {
            if !context.suggestions.is_empty() && context.show_suggestions {
                payload = payload.body(PromptBody::Raw(
                    self.formatter
                        .suggestions(&context.suggestions, self.selected_index),
                ));
            }
        }

        Ok(payload)
    }
}

#[derive(Debug)]
pub struct SuggestionContext {
    pub suggestions: Vec<String>,
    pub show_suggestions: bool,
    pub replace_range: Option<(usize, usize)>,
    pub submit_on_select: bool,
}

impl SuggestionContext {
    pub fn empty() -> Self {
        Self {
            suggestions: Vec::new(),
            show_suggestions: false,
            replace_range: None,
            submit_on_select: false,
        }
    }

    pub fn found(&self) -> bool {
        !self.suggestions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SimpleSuggester {
        suggestions: SuggesterConfig,
    }

    impl SimpleSuggester {
        fn new(suggestions: Vec<String>) -> Self {
            Self { suggestions: SuggesterConfig::new(suggestions) }
        }

        pub fn with_triggers(mut self, triggers: Vec<char>) -> Self {
            self.suggestions.trigger_chars = triggers;
            self
        }
    }

    impl Suggester for SimpleSuggester {
        fn get_suggestions(&self, input: &str, pos: usize) -> SuggestionContext {
            let input_before_cursor = &input[..pos];
            if let Some((trigger_pos, _)) = input_before_cursor
                .char_indices()
                .rev()
                .find(|(_, c)| self.suggestions.trigger_chars.contains(c))
            {
                let query = &input[trigger_pos + 1..pos].to_lowercase();
                let filtered = self
                    .suggestions
                    .suggestions
                    .iter()
                    .filter(|s| s.contains(query))
                    .take(5)
                    .cloned()
                    .collect();

                SuggestionContext {
                    suggestions: filtered,
                    replace_range: Some((trigger_pos, pos)),
                    show_suggestions: !query.is_empty(),
                    submit_on_select: self.suggestions.submit_on_select,
                }
            } else {
                SuggestionContext::empty()
            }
        }
    }

    #[test]
    fn test_autocomplete_input_basic() {
        let suggester = SimpleSuggester::new(vec!["a.rs".to_owned(), "b.rs".to_owned()]);
        let input = AutocompleteInput::new("Test").with_suggester(suggester);
        // Test initial state
        assert_eq!(input.input.value(), "");
        assert!(input.suggestion_context.is_none());
    }

    #[test]
    fn test_autocomplete_filtering() {
        let suggester = SimpleSuggester::new(vec![
            "fibo.rs".to_owned(),
            "apple.rs".to_owned(),
            "apricot.rs".to_owned(),
        ])
        .with_triggers(vec!['@']);

        let mut input = AutocompleteInput::new("Test").with_suggester(suggester);

        // Simulate typing 'a'
        input.handle(KeyCode::Char('@'), KeyModifiers::NONE);
        input.handle(KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(input.suggestion_context.is_some());

        // Simulate typing 'p'
        input.handle(KeyCode::Char('p'), KeyModifiers::NONE);
        assert!(input.suggestion_context.is_some());

        assert_eq!(
            input.suggestion_context.as_ref().unwrap().suggestions,
            vec!["apple.rs".to_string(), "apricot.rs".to_string()]
        );
    }

    #[test]
    fn test_suggestion_selection() {
        let suggester = SimpleSuggester::new(vec![
            "fibo.rs".to_owned(),
            "apple.rs".to_owned(),
            "apricot.rs".to_owned(),
        ])
        .with_triggers(vec!['@']);

        let mut input = AutocompleteInput::new("").with_suggester(suggester);
        // // Show suggestions
        input.handle(KeyCode::Char('@'), KeyModifiers::NONE);
        input.handle(KeyCode::Char('a'), KeyModifiers::NONE);
        input.handle(KeyCode::Char('p'), KeyModifiers::NONE);
        input.handle(KeyCode::Char('p'), KeyModifiers::NONE);
        input.handle(KeyCode::Tab, KeyModifiers::NONE);
        assert!(input.input.value().contains("apple.rs"));
    }
}
