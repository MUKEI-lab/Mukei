#include <QtQuickTest/quicktest.h>

#ifndef EVENT_DISPATCHER_QML_SOURCE_DIR
#  define EVENT_DISPATCHER_QML_SOURCE_DIR "."
#endif

int main(int argc, char **argv)
{
    return quick_test_main(argc, argv, "tst_EventDispatcher", EVENT_DISPATCHER_QML_SOURCE_DIR);
}
