//! Pending research manager for async research tasks.
//!
//! This module manages research tasks that run in the background while the agent
//! continues with other work. Research results are stored until they can be
//! injected into the conversation at a natural break point. Completion notifications
//! are sent via a channel for real-time UI updates.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::debug;

/// Unique identifier for a research task
pub type ResearchId = String;

/// Status of a research task
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResearchStatus {
    /// Research is in progress
    Pending,
    /// Research completed successfully
    Complete,
    /// Research failed with an error
    Failed,
}

impl std::fmt::Display for ResearchStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResearchStatus::Pending => write!(f, "pending"),
            ResearchStatus::Complete => write!(f, "complete"),
            ResearchStatus::Failed => write!(f, "failed"),
        }
    }
}

/// A research task being tracked by the manager
#[derive(Debug, Clone)]
pub struct ResearchTask {
    /// Unique identifier for this research task
    pub id: ResearchId,
    /// The original research query
    pub query: String,
    /// Current status of the research
    pub status: ResearchStatus,
    /// The research result (report or error message)
    pub result: Option<String>,
    /// When the research was initiated
    pub started_at: Instant,
    /// Whether this result has been injected into the conversation
    pub injected: bool,
}

impl ResearchTask {
    /// Create a new pending research task
    pub fn new(id: ResearchId, query: String) -> Self {
        Self {
            id,
            query,
            status: ResearchStatus::Pending,
            result: None,
            started_at: Instant::now(),
            injected: false,
        }
    }

    /// Get the elapsed time since the research started
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Format elapsed time for display
    pub fn elapsed_display(&self) -> String {
        let secs = self.elapsed().as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m {}s", secs / 60, secs % 60)
        }
    }
}

/// Notification sent when a research task completes (success or failure)
#[derive(Debug, Clone)]
pub struct ResearchCompletionNotification {
    /// The research ID that completed
    pub id: ResearchId,
    /// Whether it succeeded or failed
    pub status: ResearchStatus,
    /// The query that was researched
    pub query: String,
}

/// Thread-safe manager for pending research tasks
#[derive(Debug, Clone)]
pub struct PendingResearchManager {
    tasks: Arc<Mutex<HashMap<ResearchId, ResearchTask>>>,
    /// Channel sender for completion notifications (optional, for UI updates)
    completion_tx: Option<tokio::sync::broadcast::Sender<ResearchCompletionNotification>>,
}

impl Default for PendingResearchManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingResearchManager {
    /// Create a new pending research manager
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            completion_tx: None,
        }
    }

    /// Create a new pending research manager with completion notifications enabled.
    ///
    /// Returns the manager and a receiver for completion notifications.
    /// The receiver can be used to get real-time updates when research completes.
    pub fn with_notifications() -> (Self, tokio::sync::broadcast::Receiver<ResearchCompletionNotification>) {
        // Buffer size of 16 should be plenty for concurrent research tasks
        let (tx, rx) = tokio::sync::broadcast::channel(16);
        let manager = Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            completion_tx: Some(tx),
        };
        (manager, rx)
    }

    /// Subscribe to completion notifications.
    ///
    /// Returns None if notifications are not enabled (manager created with `new()`).
    pub fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<ResearchCompletionNotification>> {
        self.completion_tx.as_ref().map(|tx| tx.subscribe())
    }

    /// Generate a unique research ID
    pub fn generate_id() -> ResearchId {
        use std::time::{SystemTime, UNIX_EPOCH};
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("research_{:x}_{:08x}", timestamp, counter)
    }

    /// Register a new research task
    ///
    /// Returns the research ID for tracking
    pub fn register(&self, query: &str) -> ResearchId {
        let id = Self::generate_id();
        let task = ResearchTask::new(id.clone(), query.to_string());
        
        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(id.clone(), task);
        
        debug!("Registered research task: {} for query: {}", id, query);
        id
    }

    /// Update a research task with its result
    pub fn complete(&self, id: &ResearchId, result: String) {
        let notification = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(id) {
                task.status = ResearchStatus::Complete;
                task.result = Some(result);
                debug!("Research task {} completed successfully", id);
                Some(ResearchCompletionNotification {
                    id: id.clone(),
                    status: ResearchStatus::Complete,
                    query: task.query.clone(),
                })
            } else {
                None
            }
        };
        // Send notification outside the lock to avoid potential deadlocks
        if let (Some(notification), Some(tx)) = (notification, &self.completion_tx) {
            let _ = tx.send(notification); // Ignore error if no receivers
        }
    }

    /// Mark a research task as failed
    pub fn fail(&self, id: &ResearchId, error: String) {
        let notification = {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(id) {
                task.status = ResearchStatus::Failed;
                task.result = Some(error);
                debug!("Research task {} failed", id);
                Some(ResearchCompletionNotification {
                    id: id.clone(),
                    status: ResearchStatus::Failed,
                    query: task.query.clone(),
                })
            } else {
                None
            }
        };
        // Send notification outside the lock to avoid potential deadlocks
        if let (Some(notification), Some(tx)) = (notification, &self.completion_tx) {
            let _ = tx.send(notification); // Ignore error if no receivers
        }
    }

    /// Get the status of a specific research task
    pub fn get_status(&self, id: &ResearchId) -> Option<(ResearchStatus, Option<String>)> {
        let tasks = self.tasks.lock().unwrap();
        tasks.get(id).map(|t| (t.status.clone(), t.result.clone()))
    }

    /// Get a specific research task
    pub fn get(&self, id: &ResearchId) -> Option<ResearchTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.get(id).cloned()
    }

    /// List all pending (not yet injected) research tasks
    pub fn list_pending(&self) -> Vec<ResearchTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .values()
            .filter(|t| !t.injected)
            .cloned()
            .collect()
    }

    /// List all research tasks (including injected ones)
    pub fn list_all(&self) -> Vec<ResearchTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.values().cloned().collect()
    }

    /// Get count of pending (in-progress) research tasks
    pub fn pending_count(&self) -> usize {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .values()
            .filter(|t| t.status == ResearchStatus::Pending)
            .count()
    }

    /// Get count of completed but not yet injected research tasks
    pub fn ready_count(&self) -> usize {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .values()
            .filter(|t| !t.injected && t.status != ResearchStatus::Pending)
            .count()
    }

    /// Take all completed research tasks that haven't been injected yet
    ///
    /// Marks them as injected so they won't be returned again
    pub fn take_completed(&self) -> Vec<ResearchTask> {
        let mut tasks = self.tasks.lock().unwrap();
        let mut completed = Vec::new();
        
        for task in tasks.values_mut() {
            if !task.injected && task.status != ResearchStatus::Pending {
                task.injected = true;
                completed.push(task.clone());
            }
        }
        
        debug!("Took {} completed research tasks for injection", completed.len());
        completed
    }

    /// Remove a research task (e.g., after it's been fully processed)
    pub fn remove(&self, id: &ResearchId) -> Option<ResearchTask> {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.remove(id)
    }

    /// Clear all completed and injected tasks (cleanup)
    pub fn cleanup_injected(&self) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.retain(|_, t| !t.injected);
    }

    /// Check if there are any tasks (pending or ready)
    pub fn has_tasks(&self) -> bool {
        let tasks = self.tasks.lock().unwrap();
        !tasks.is_empty()
    }

    /// Format a summary of pending research for display
    pub fn format_status_summary(&self) -> Option<String> {
        let tasks = self.tasks.lock().unwrap();
        
        let pending: Vec<_> = tasks.values().filter(|t| t.status == ResearchStatus::Pending).collect();
        let ready: Vec<_> = tasks.values().filter(|t| !t.injected && t.status != ResearchStatus::Pending).collect();
        
        if pending.is_empty() && ready.is_empty() {
            return None;
        }
        
        let mut parts = Vec::new();
        
        if !pending.is_empty() {
            parts.push(format!("🔍 {} researching", pending.len()));
        }
        
        if !ready.is_empty() {
            parts.push(format!("📋 {} ready", ready.len()));
        }
        
        Some(parts.join(" | "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_register_and_get() {
        let manager = PendingResearchManager::new();
        
        let id = manager.register("How to use tokio?");
        
        let task = manager.get(&id).unwrap();
        assert_eq!(task.query, "How to use tokio?");
        assert_eq!(task.status, ResearchStatus::Pending);
        assert!(task.result.is_none());
        assert!(!task.injected);
    }

    #[test]
    fn test_complete_research() {
        let manager = PendingResearchManager::new();
        
        let id = manager.register("Test query");
        manager.complete(&id, "Research report here".to_string());
        
        let (status, result) = manager.get_status(&id).unwrap();
        assert_eq!(status, ResearchStatus::Complete);
        assert_eq!(result.unwrap(), "Research report here");
    }

    #[test]
    fn test_fail_research() {
        let manager = PendingResearchManager::new();
        
        let id = manager.register("Test query");
        manager.fail(&id, "Connection timeout".to_string());
        
        let (status, result) = manager.get_status(&id).unwrap();
        assert_eq!(status, ResearchStatus::Failed);
        assert_eq!(result.unwrap(), "Connection timeout");
    }

    #[test]
    fn test_take_completed() {
        let manager = PendingResearchManager::new();
        
        let id1 = manager.register("Query 1");
        let id2 = manager.register("Query 2");
        let id3 = manager.register("Query 3");
        
        // Complete two, leave one pending
        manager.complete(&id1, "Report 1".to_string());
        manager.fail(&id2, "Error".to_string());
        // id3 stays pending
        
        // Take completed
        let completed = manager.take_completed();
        assert_eq!(completed.len(), 2);
        
        // Taking again should return empty (already injected)
        let completed_again = manager.take_completed();
        assert!(completed_again.is_empty());
        
        // Pending count should be 1
        assert_eq!(manager.pending_count(), 1);
    }

    #[test]
    fn test_list_pending() {
        let manager = PendingResearchManager::new();
        
        let id1 = manager.register("Query 1");
        let id2 = manager.register("Query 2");
        
        manager.complete(&id1, "Report".to_string());
        
        // Both should be in list_pending (not injected yet)
        let pending = manager.list_pending();
        assert_eq!(pending.len(), 2);
        
        // Take completed
        manager.take_completed();
        
        // Now only the actually pending one should be listed
        let pending = manager.list_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id2);
    }

    #[test]
    fn test_format_status_summary() {
        let manager = PendingResearchManager::new();
        
        // Empty - no summary
        assert!(manager.format_status_summary().is_none());
        
        // One pending
        let id1 = manager.register("Query 1");
        let summary = manager.format_status_summary().unwrap();
        assert!(summary.contains("1 researching"));
        
        // One pending, one ready
        let id2 = manager.register("Query 2");
        manager.complete(&id2, "Report".to_string());
        let summary = manager.format_status_summary().unwrap();
        assert!(summary.contains("1 researching"));
        assert!(summary.contains("1 ready"));
    }

    #[test]
    fn test_thread_safety() {
        let manager = PendingResearchManager::new();
        let manager_clone = manager.clone();
        
        let id = manager.register("Concurrent test");
        let id_clone = id.clone();
        
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            manager_clone.complete(&id_clone, "Result from thread".to_string());
        });
        
        // Main thread checks status
        loop {
            if let Some((status, _)) = manager.get_status(&id) {
                if status == ResearchStatus::Complete {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        
        handle.join().unwrap();
        
        let (status, result) = manager.get_status(&id).unwrap();
        assert_eq!(status, ResearchStatus::Complete);
        assert_eq!(result.unwrap(), "Result from thread");
    }

    #[test]
    fn test_elapsed_display() {
        let manager = PendingResearchManager::new();
        let id = manager.register("Test");
        
        let task = manager.get(&id).unwrap();
        let display = task.elapsed_display();
        
        // Should be "0s" or similar (just started)
        assert!(display.contains('s'));
    }

    #[test]
    fn test_cleanup_injected() {
        let manager = PendingResearchManager::new();
        
        let id1 = manager.register("Query 1");
        let id2 = manager.register("Query 2");
        
        manager.complete(&id1, "Report 1".to_string());
        manager.complete(&id2, "Report 2".to_string());
        
        // Take and inject
        manager.take_completed();
        
        // Both should still exist
        assert_eq!(manager.list_all().len(), 2);
        
        // Cleanup injected
        manager.cleanup_injected();
        
        // Should be empty now
        assert_eq!(manager.list_all().len(), 0);
    }

    #[test]
    fn test_generate_id_uniqueness() {
        let ids: Vec<_> = (0..100).map(|_| PendingResearchManager::generate_id()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "Generated IDs should be unique");
    }

    #[tokio::test]
    async fn test_notifications_on_complete() {
        let (manager, mut rx) = PendingResearchManager::with_notifications();
        
        let id = manager.register("Test query");
        
        // Complete the research
        manager.complete(&id, "Report content".to_string());
        
        // Should receive a notification
        let notification = rx.recv().await.unwrap();
        assert_eq!(notification.id, id);
        assert_eq!(notification.status, ResearchStatus::Complete);
        assert_eq!(notification.query, "Test query");
    }

    #[tokio::test]
    async fn test_notifications_on_fail() {
        let (manager, mut rx) = PendingResearchManager::with_notifications();
        
        let id = manager.register("Test query");
        
        // Fail the research
        manager.fail(&id, "Connection error".to_string());
        
        // Should receive a notification
        let notification = rx.recv().await.unwrap();
        assert_eq!(notification.id, id);
        assert_eq!(notification.status, ResearchStatus::Failed);
        assert_eq!(notification.query, "Test query");
    }

    #[test]
    fn test_no_notifications_without_setup() {
        let manager = PendingResearchManager::new();
        // subscribe() should return None when notifications not enabled
        assert!(manager.subscribe().is_none());
    }
}
