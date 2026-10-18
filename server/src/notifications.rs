//! Organizer notifications, modelled after Laravel's notification system:
//! each notification type decides which channels it goes `via` (in-app database
//! notifications and/or email), and knows how to render itself for each channel.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::config::Config;
use crate::error::AppResult;
use crate::mail::{self, templates, OutgoingEmail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Stored in `notifications`, shown in the dashboard bell.
    Database,
    /// Queued in the email outbox.
    Mail,
}

pub trait Notification: Send + Sync {
    fn kind(&self) -> &'static str;
    fn game_id(&self) -> Option<Uuid>;
    fn title(&self) -> String;
    fn body(&self) -> String;

    /// In-app path the notification links to.
    fn link(&self) -> String {
        self.game_id()
            .map(|id| format!("/games/{id}"))
            .unwrap_or_else(|| "/dashboard".into())
    }

    /// Important notifications are emailed with the default preferences.
    fn important(&self) -> bool {
        false
    }

    /// Channels to deliver on, given the organizer's email preference
    /// (`all`, `important` or `none`). In-app delivery is always on.
    fn via(&self, email_preference: &str) -> Vec<Channel> {
        let mut channels = vec![Channel::Database];
        let email = match email_preference {
            "all" => true,
            "important" => self.important(),
            _ => false,
        };
        if email {
            channels.push(Channel::Mail);
        }
        channels
    }

    fn to_mail(&self, config: &Config) -> templates::Rendered {
        templates::notification(&self.title(), &self.body(), &config.url(&self.link()))
    }
}

/// Deliver a notification to an organizer on every channel it goes via.
pub async fn notify(
    conn: &mut PgConnection,
    config: &Config,
    organizer_id: Uuid,
    notification: &dyn Notification,
) -> AppResult<()> {
    let (email, preference): (String, String) =
        sqlx::query_as("SELECT email, email_notifications FROM organizers WHERE id = $1")
            .bind(organizer_id)
            .fetch_one(&mut *conn)
            .await?;

    for channel in notification.via(&preference) {
        match channel {
            Channel::Database => {
                sqlx::query(
                    "INSERT INTO notifications (organizer_id, game_id, kind, title, body, link)
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(organizer_id)
                .bind(notification.game_id())
                .bind(notification.kind())
                .bind(notification.title())
                .bind(notification.body())
                .bind(notification.link())
                .execute(&mut *conn)
                .await?;
            }
            Channel::Mail => {
                let rendered = notification.to_mail(config);
                mail::enqueue(
                    &mut *conn,
                    &OutgoingEmail {
                        kind: "notification",
                        to: email.clone(),
                        subject: rendered.subject,
                        html: rendered.html,
                        text: rendered.text,
                        reply_to: None,
                        organizer_id: Some(organizer_id),
                        game_id: notification.game_id(),
                        participant_id: None,
                    },
                )
                .await?;
            }
        }
    }
    Ok(())
}

fn photos(n: i64) -> String {
    if n == 1 {
        "1 photo".into()
    } else {
        format!("{n} photos")
    }
}

pub struct PhotoUploaded {
    pub game_id: Uuid,
    pub game_name: String,
    pub participant: String,
    pub uploaded: i64,
    pub required: i32,
}

impl Notification for PhotoUploaded {
    fn kind(&self) -> &'static str {
        "photo_uploaded"
    }
    fn game_id(&self) -> Option<Uuid> {
        Some(self.game_id)
    }
    fn title(&self) -> String {
        format!("{} uploaded a photo", self.participant)
    }
    fn body(&self) -> String {
        format!(
            "{} has now shared {} of {} for “{}”.",
            self.participant,
            self.uploaded,
            photos(self.required as i64),
            self.game_name
        )
    }
}

pub struct ParticipantReady {
    pub game_id: Uuid,
    pub game_name: String,
    pub participant: String,
    pub declared_done: bool,
    pub photos: i64,
    pub ready: i64,
    pub total: i64,
}

impl Notification for ParticipantReady {
    fn kind(&self) -> &'static str {
        "participant_ready"
    }
    fn game_id(&self) -> Option<Uuid> {
        Some(self.game_id)
    }
    fn title(&self) -> String {
        format!("{} is ready", self.participant)
    }
    fn body(&self) -> String {
        let how = if self.declared_done {
            format!("stopped after {}", photos(self.photos))
        } else {
            format!("shared all {}", photos(self.photos))
        };
        format!(
            "{} {how} for “{}”. {} of {} participants are ready.",
            self.participant, self.game_name, self.ready, self.total
        )
    }
}

pub struct CollectionComplete {
    pub game_id: Uuid,
    pub game_name: String,
    pub total: i64,
    pub photos: i64,
}

impl Notification for CollectionComplete {
    fn kind(&self) -> &'static str {
        "collection_complete"
    }
    fn game_id(&self) -> Option<Uuid> {
        Some(self.game_id)
    }
    fn important(&self) -> bool {
        true
    }
    fn title(&self) -> String {
        format!("Everyone is ready for “{}”", self.game_name)
    }
    fn body(&self) -> String {
        format!(
            "All {} participants are done: {} collected. You can mark the game as ready and share the play link.",
            self.total,
            photos(self.photos)
        )
    }
}

pub struct DeadlineReached {
    pub game_id: Uuid,
    pub game_name: String,
    pub ready: i64,
    pub total: i64,
    pub photos: i64,
}

impl Notification for DeadlineReached {
    fn kind(&self) -> &'static str {
        "deadline_reached"
    }
    fn game_id(&self) -> Option<Uuid> {
        Some(self.game_id)
    }
    fn important(&self) -> bool {
        true
    }
    fn title(&self) -> String {
        format!("Deadline reached for “{}”", self.game_name)
    }
    fn body(&self) -> String {
        format!(
            "{} of {} participants are ready and {} have been collected. Late uploads stay open until you mark the game as ready.",
            self.ready,
            self.total,
            photos(self.photos)
        )
    }
}

pub struct TeamFinished {
    pub game_id: Uuid,
    pub game_name: String,
    pub team: String,
    pub score: i64,
}

impl Notification for TeamFinished {
    fn kind(&self) -> &'static str {
        "team_finished"
    }
    fn game_id(&self) -> Option<Uuid> {
        Some(self.game_id)
    }
    fn title(&self) -> String {
        format!("Team {} finished", self.team)
    }
    fn body(&self) -> String {
        format!(
            "Team {} finished “{}” with {} points.",
            self.team, self.game_name, self.score
        )
    }
}

pub struct EmailFailed {
    pub game_id: Option<Uuid>,
    pub recipient: String,
    pub error: String,
}

impl Notification for EmailFailed {
    fn kind(&self) -> &'static str {
        "email_failed"
    }
    fn game_id(&self) -> Option<Uuid> {
        self.game_id
    }
    fn title(&self) -> String {
        format!("Email to {} failed", self.recipient)
    }
    fn body(&self) -> String {
        format!(
            "We couldn't deliver an email to {} ({}). You can copy their personal link from the game page and send it another way.",
            self.recipient, self.error
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_follow_preferences() {
        let important = CollectionComplete {
            game_id: Uuid::nil(),
            game_name: "g".into(),
            total: 3,
            photos: 4,
        };
        let minor = TeamFinished {
            game_id: Uuid::nil(),
            game_name: "g".into(),
            team: "t".into(),
            score: 1,
        };
        assert_eq!(
            important.via("important"),
            vec![Channel::Database, Channel::Mail]
        );
        assert_eq!(minor.via("important"), vec![Channel::Database]);
        assert_eq!(minor.via("all"), vec![Channel::Database, Channel::Mail]);
        assert_eq!(important.via("none"), vec![Channel::Database]);
    }
}
