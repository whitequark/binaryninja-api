#pragma once

#include "uitypes.h"
#include <QString>
#include <QObject>
#include <QVersionNumber>
#include <QDateTime>
#include <QDialog>
#include "binaryninjaapi.h"

class BINARYNINJAUIAPI UpdateInfoFetcher : public QObject
{
	Q_OBJECT

public:
	struct Version
	{
		QString versionStringToGiveToMainWindow;
		QVersionNumber version;
		QDateTime date;
		bool isCurrent = false;
		Version(BNUpdateVersionNew);
	};
	struct ChangelogEntryItem
	{
		QString author;
		QString commit;
		QString body;
		ChangelogEntryItem(const QString& author = "", const QString& commit = "", const QString& body = "")
			: author(author), commit(commit), body(body) {};
		/// In-struct cache for wrapped text
		mutable QString bodyWrapCache;
	};
	struct ChangelogEntry
	{
		QVersionNumber version;
		QDateTime date;
		bool isNew = false;
		std::vector<ChangelogEntryItem> entryItems;
		ChangelogEntry(BNChangelogEntry);
	};
	struct Channel
	{
		QString name;
		QString description;
		std::vector<Version> versions;
		std::vector<ChangelogEntry> changelog;
		Channel(BNUpdateChannelFullInfo);
		Channel() {};
	};
	enum FetchError
	{
		NoError,
		ConnectionError,
		DeserError
	};

private:
	std::vector<Channel> m_channels;
	std::mutex m_infoMutex;
	std::atomic<bool> m_fetchStarted;
	FetchError m_fetchError;
	std::atomic<bool> m_done;

	UpdateInfoFetcher() {};
	void _startFetch();

public:
	static UpdateInfoFetcher* Instance();
	static bool done() { return Instance()->m_done; }
	static FetchError fetchError() { return Instance()->m_fetchError; }
	static bool fetchStarted() { return Instance()->m_fetchStarted; }
	static void startFetch() { Instance()->_startFetch(); }
	static const std::vector<Channel>& getChannels();
	static const Channel* getActiveChannel();
signals:
	void fetchCompleted(const FetchError& error);
};

class BINARYNINJAUIAPI UpdateInfoCommitFinder : public QDialog
{
	Q_OBJECT

public:
	UpdateInfoCommitFinder(QWidget* parent);
protected:
	void resizeEvent(QResizeEvent* event) override;
};
