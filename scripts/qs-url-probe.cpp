// Loads a QML file the way `quickshell -p` does (quickshell 0.3.2).
#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QIODevice>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QQmlAbstractUrlInterceptor>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQmlNetworkAccessManagerFactory>
#include <QTextStream>
#include <QUrl>

class Interceptor final : public QQmlAbstractUrlInterceptor {
public:
    explicit Interceptor(const QDir& configRoot) : configRoot(configRoot) {}

    QUrl intercept(const QUrl& originalUrl, DataType type) override {
        auto url = originalUrl;
        if (url.scheme() == QStringLiteral("root")) {
            url.setScheme(QStringLiteral("qs"));
            auto path = url.path();
            if (path.startsWith(QLatin1Char('/'))) path = path.sliced(1);
            url.setPath(QStringLiteral("@/qs/") + path);
        }
        if (url.scheme() != QStringLiteral("qs")) return url;

        auto path = url.path();
        if (path.startsWith(QStringLiteral("@/qs/"))) {
            path = configRoot.filePath(path.sliced(5));
        } else if (!path.startsWith(QLatin1Char('/'))) {
            return QUrl(QStringLiteral("qrc:/qs-blackhole"));
        }

        if (type == DataType::UrlString) {
            if (path.endsWith(QStringLiteral(".qml"))) return url;
            auto rewritten = url;
            rewritten.setScheme(QStringLiteral("file"));
            rewritten.setPath(path);
            return rewritten;
        }
        return url;
    }

private:
    QDir configRoot;
};

class Nam final : public QNetworkAccessManager {
public:
    Nam(const QDir& configRoot, QObject* parent)
        : QNetworkAccessManager(parent), configRoot(configRoot) {}

    QNetworkReply* createRequest(Operation op, const QNetworkRequest& req, QIODevice* outgoing) override {
        auto url = req.url();
        if (url.scheme() != QStringLiteral("qs")) {
            return QNetworkAccessManager::createRequest(op, req, outgoing);
        }
        auto path = url.path();
        if (path.startsWith(QStringLiteral("@/qs/"))) path = configRoot.filePath(path.sliced(5));
        auto fileReq = req;
        auto fileUrl = url;
        fileUrl.setScheme(QStringLiteral("file"));
        fileUrl.setPath(path);
        fileReq.setUrl(fileUrl);
        return QNetworkAccessManager::createRequest(op, fileReq, outgoing);
    }

private:
    QDir configRoot;
};

class NamFactory final : public QQmlNetworkAccessManagerFactory {
public:
    explicit NamFactory(const QDir& configRoot) : configRoot(configRoot) {}
    QNetworkAccessManager* create(QObject* parent) override { return new Nam(configRoot, parent); }

private:
    QDir configRoot;
};

int main(int argc, char** argv) {
    QGuiApplication app(argc, argv);
    if (argc < 2) {
        QTextStream(stderr) << "usage: qs-url-probe <qml-file> [config-root]\n";
        return 2;
    }

    const QFileInfo rootFile(QString::fromLocal8Bit(argv[1]));
    const QDir configRoot(argc >= 3 ? QString::fromLocal8Bit(argv[2]) : rootFile.dir().absolutePath());
    const QString rel = configRoot.relativeFilePath(rootFile.absoluteFilePath());
    Interceptor interceptor(configRoot);
    NamFactory namFactory(configRoot);

    QQmlEngine engine;
    QObject::connect(&engine, &QQmlEngine::quit, &app, &QCoreApplication::quit);
    engine.addUrlInterceptor(&interceptor);
    engine.addImportPath(QStringLiteral("qs:@/"));
    engine.setNetworkAccessManagerFactory(&namFactory);

    QUrl url;
    url.setScheme(QStringLiteral("qs"));
    url.setPath(QStringLiteral("@/qs/") + rel);

    QTextStream out(stdout);
    out << "configRoot=" << configRoot.absolutePath() << '\n';
    out << "documentUrl=" << url.toString() << '\n';
    out.flush();

    QQmlComponent component(&engine, url);
    if (!component.isReady()) {
        QTextStream(stderr) << component.errorString() << '\n';
        return 1;
    }
    QObject* root = component.create();
    if (root == nullptr) {
        QTextStream(stderr) << component.errorString() << '\n';
        return 1;
    }
    return 0;
}
