#include <QtQuickTest/quicktest.h>

#ifndef QML_ARCHITECTURE_TEST_SOURCE_DIR
#  define QML_ARCHITECTURE_TEST_SOURCE_DIR "."
#endif

int main(int argc, char **argv)
{
    return quick_test_main(argc, argv, "tst_QmlArchitecture", QML_ARCHITECTURE_TEST_SOURCE_DIR);
}
